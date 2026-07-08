use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

use crate::{adapters, paths, safety, secrets, store, targeting};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunPlan {
    pub run_id: String,
    pub target_id: String,
    pub benchmark_pack_id: String,
    pub task_id: String,
    pub workspace_path: PathBuf,
    pub artifact_path: PathBuf,
    pub timeout_seconds: u64,
    pub sandbox: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResultDto {
    pub id: String,
    #[serde(rename = "targetId")]
    pub target_id: String,
    #[serde(rename = "benchmarkPackId")]
    pub benchmark_pack_id: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub status: String,
    pub score: Option<f64>,
    #[serde(rename = "wallTimeMs")]
    pub wall_time_ms: u64,
    pub artifacts: Vec<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunQuickSmokeRequest {
    #[serde(rename = "targetIds")]
    pub target_ids: Vec<String>,
    #[serde(default = "default_pack")]
    #[serde(rename = "benchmarkPackId")]
    pub benchmark_pack_id: String,
    #[serde(default)]
    #[serde(rename = "taskIds")]
    pub task_ids: Vec<String>,
    #[serde(default = "default_repetitions")]
    pub repetitions: u32,
    #[serde(default)]
    pub docker: bool,
    #[serde(default)]
    #[serde(rename = "warmupRuns")]
    pub warmup_runs: u32,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    #[serde(default, rename = "maxCostUsd")]
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    #[serde(rename = "runGroupId")]
    pub run_group_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunProgressDto {
    pub total: usize,
    pub completed: usize,
    #[serde(rename = "currentTargetId")]
    pub current_target_id: Option<String>,
    #[serde(rename = "currentTaskId")]
    pub current_task_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BenchmarkPackSpec {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub estimated_runtime: Option<String>,
    #[serde(default)]
    pub requires_sandbox: bool,
    #[serde(default)]
    pub calibration: Option<BenchmarkPackCalibrationSpec>,
    pub tasks: Vec<String>,
    #[serde(skip)]
    pub pack_dir: PathBuf,
    #[serde(skip)]
    pub pack_path: PathBuf,
    #[serde(skip)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkPackCalibrationSpec {
    #[serde(default = "default_calibration_status")]
    pub status: String,
    #[serde(default)]
    pub sample_size: Option<u64>,
    #[serde(default)]
    pub baseline_models: Vec<String>,
    #[serde(default)]
    pub last_reviewed: Option<String>,
    #[serde(default)]
    pub review_scope: Option<String>,
    #[serde(default)]
    pub quality_gates: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub task_type: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub fixture: Option<String>,
    pub prompt: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub max_turns: Option<u32>,
    #[serde(default = "default_task_weight")]
    pub weight: f64,
    pub scoring: ScoringSpec,
    #[serde(skip)]
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringSpec {
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub parse: Option<String>,
    #[serde(default)]
    pub expect_exact: Option<String>,
    #[serde(default)]
    pub expect_contains: Vec<String>,
    #[serde(default)]
    pub expect_regex: Vec<String>,
    #[serde(default)]
    pub expect_not_contains: Vec<String>,
    #[serde(default)]
    pub expect_json: bool,
    #[serde(default)]
    pub json_field_equals: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub json_field_contains: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub json_field_object_keys_exact: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub json_field_array_exact: HashMap<String, Vec<serde_json::Value>>,
    #[serde(default)]
    pub json_field_array_exact_ordered: HashMap<String, Vec<serde_json::Value>>,
    #[serde(default)]
    pub json_field_number_close: HashMap<String, JsonNumberCloseSpec>,
    #[serde(default)]
    pub json_field_number_bounds: HashMap<String, JsonNumberBoundsSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonNumberCloseSpec {
    pub expected: f64,
    #[serde(default = "default_number_tolerance")]
    pub tolerance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonNumberBoundsSpec {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkPackDto {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub estimated_runtime: Option<String>,
    pub requires_sandbox: bool,
    pub tasks: usize,
    pub prompt_tasks: usize,
    pub total_task_weight: f64,
    pub heavy: bool,
    pub task_types: Vec<String>,
    pub languages: Vec<String>,
    pub required_tools: Vec<String>,
    pub scoring_methods: Vec<String>,
    pub supported_target_kinds: Vec<String>,
    pub target_fit: String,
    pub evidence_profile: String,
    pub evidence_warnings: Vec<String>,
    pub calibration_status: String,
    pub calibration_sample_size: Option<u64>,
    pub calibration_baseline_models: Vec<String>,
    pub calibration_last_reviewed: Option<String>,
    pub calibration_review_scope: Option<String>,
    pub calibration_quality_gates: Vec<String>,
    pub calibration_notes: Option<String>,
    pub source: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkPackDiagnosticDto {
    pub id: Option<String>,
    pub source: String,
    pub source_path: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkPackTaskDto {
    pub id: String,
    pub name: String,
    pub task_type: String,
    pub language: Option<String>,
    pub fixture: Option<String>,
    pub prompt: String,
    pub timeout_seconds: u64,
    pub max_turns: Option<u32>,
    pub weight: f64,
    pub scoring_methods: Vec<String>,
    pub scoring: serde_json::Value,
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBenchmarkPackTemplateRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub prompt: String,
    pub expected_response: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBenchmarkPackTemplateDto {
    pub pack: BenchmarkPackDto,
    pub source_path: String,
    pub task_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddBenchmarkPackPromptTaskRequest {
    pub pack_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub scoring_method: String,
    #[serde(default)]
    pub expected_response: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedBenchmarkPackPromptTaskDto {
    pub pack: BenchmarkPackDto,
    pub source_path: String,
    pub task_path: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBenchmarkPackPromptTaskRequest {
    pub pack_id: String,
    pub task_id: String,
    pub name: String,
    pub prompt: String,
    pub scoring_method: String,
    #[serde(default)]
    pub expected_response: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedBenchmarkPackPromptTaskDto {
    pub pack: BenchmarkPackDto,
    pub source_path: String,
    pub task_path: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBenchmarkPackCalibrationRequest {
    pub pack_id: String,
    pub status: String,
    #[serde(default)]
    pub sample_size: Option<u64>,
    #[serde(default)]
    pub baseline_models: Vec<String>,
    #[serde(default)]
    pub last_reviewed: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedBenchmarkPackCalibrationDto {
    pub pack: BenchmarkPackDto,
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScorePromptTaskPreviewRequest {
    pub scoring_method: String,
    #[serde(default)]
    pub expected_response: Option<String>,
    pub sample_response: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScorePromptTaskPreviewDto {
    pub status: String,
    pub score: f64,
    pub tests: serde_json::Value,
    pub error_message: Option<String>,
    pub scoring_methods: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBenchmarkPackTaskRequest {
    pub pack_id: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedBenchmarkPackTaskDto {
    pub pack: BenchmarkPackDto,
    pub source_path: String,
    pub deleted_task_id: String,
    pub deleted_task_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportBenchmarkPackRequest {
    pub pack_id: String,
    #[serde(default)]
    pub destination_dir: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedBenchmarkPackDto {
    pub pack: BenchmarkPackDto,
    pub source_path: String,
    pub export_path: String,
    pub format: String,
    pub files_copied: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBenchmarkPackRequest {
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedBenchmarkPackDto {
    pub pack: BenchmarkPackDto,
    pub source_path: String,
    pub import_path: String,
    pub files_copied: usize,
}

#[derive(Debug)]
pub struct CommandCapture {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub timed_out: bool,
    pub wall_time_ms: u64,
    pub peak_rss_mb: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DockerScoringImageMetadata {
    image: String,
    image_id: Option<String>,
    image_digest: Option<String>,
    repo_digests: Vec<String>,
    dockerfile_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ScoringCommandMetadata {
    command: Vec<String>,
    resolved_command: Option<String>,
    version_probe: Option<Vec<String>>,
    version_stdout: Option<String>,
    version_stderr: Option<String>,
    version_exit_code: Option<i32>,
    version_timed_out: bool,
}

#[derive(Debug)]
struct CliAgentRun {
    capture: CommandCapture,
    command_metadata: ScoringCommandMetadata,
    working_dir: PathBuf,
    env: BTreeMap<String, String>,
}

#[derive(Debug)]
struct CliAgentEvidenceArtifacts {
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    command_path: PathBuf,
    evidence: serde_json::Value,
    stdout_bytes: usize,
    stderr_bytes: usize,
}

struct StreamCommandCapture {
    response: ProviderStreamResponse,
    stream_error: Option<String>,
    stderr: String,
    code: Option<i32>,
    timed_out: bool,
}

struct ModelClientOutput {
    content: String,
    raw_response: Option<String>,
    metrics: serde_json::Map<String, serde_json::Value>,
}

struct ModelExecutionError {
    message: String,
    output: Option<ModelClientOutput>,
}

impl ModelExecutionError {
    fn without_output(message: String) -> Self {
        Self {
            message,
            output: None,
        }
    }

    fn with_output(message: String, output: ModelClientOutput) -> Self {
        Self {
            message,
            output: Some(output),
        }
    }
}

const BENCHMARK_PROMPT_SYSTEM: &str =
    "Answer the benchmark prompt directly. Do not include hidden reasoning.";
const CODE_EDIT_SYSTEM: &str =
    "Return either a unified diff or JSON file edits as {\"edits\":[{\"path\":\"relative\",\"content\":\"...\"}]}.";

struct ModelClientRequest<'a> {
    task_id: &'a str,
    system_prompt: Cow<'a, str>,
    user_prompt: Cow<'a, str>,
    default_max_tokens: u64,
}

impl<'a> ModelClientRequest<'a> {
    fn benchmark_prompt(task: &'a TaskSpec) -> Self {
        Self {
            task_id: &task.id,
            system_prompt: Cow::Borrowed(BENCHMARK_PROMPT_SYSTEM),
            user_prompt: Cow::Borrowed(&task.prompt),
            default_max_tokens: 512,
        }
    }

    fn code_edit(task: &'a TaskSpec, workspace: &Path) -> Self {
        Self {
            task_id: &task.id,
            system_prompt: Cow::Borrowed(CODE_EDIT_SYSTEM),
            user_prompt: Cow::Owned(format!(
                "{}\n\nWorkspace files:\n{}",
                task.prompt,
                list_workspace_files(workspace)
            )),
            default_max_tokens: 4096,
        }
    }
}

enum ModelClient {
    Mock,
    OpenAiCompatible {
        adapter: adapters::AdapterSpec,
        config: serde_json::Value,
    },
    OpenAiResponses {
        adapter: adapters::AdapterSpec,
        config: serde_json::Value,
    },
    AnthropicMessages {
        adapter: adapters::AdapterSpec,
        config: serde_json::Value,
    },
    AzureOpenAi {
        adapter: adapters::AdapterSpec,
        config: serde_json::Value,
    },
}

impl ModelClient {
    fn for_target(target: &store::TargetRecord) -> Result<Self, String> {
        match target.kind.as_str() {
            "mock" => Ok(Self::Mock),
            "direct_model" | "harnessed_model" => {
                let Some(adapter) = adapters::find_adapter(&target.adapter_id)? else {
                    return Err(format!("adapter {} not found", target.adapter_id));
                };
                let config: serde_json::Value = serde_json::from_str(&target.config_json)
                    .unwrap_or_else(|_| serde_json::json!({}));
                match adapter.spec.kind.as_str() {
                    "openai_responses" => Ok(Self::OpenAiResponses {
                        adapter: adapter.spec,
                        config,
                    }),
                    "openai_compatible" | "mistral_api" => Ok(Self::OpenAiCompatible {
                        adapter: adapter.spec,
                        config,
                    }),
                    "anthropic_messages" => Ok(Self::AnthropicMessages {
                        adapter: adapter.spec,
                        config,
                    }),
                    "azure_openai" => Ok(Self::AzureOpenAi {
                        adapter: adapter.spec,
                        config,
                    }),
                    other => Err(format!("provider_skipped: {} has no model client", other)),
                }
            }
            other => Err(format!("unsupported model target kind: {}", other)),
        }
    }

    fn complete(
        &self,
        request: &ModelClientRequest<'_>,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<ModelClientOutput, String> {
        match self {
            Self::Mock => Ok(mock_prompt_output(request)),
            Self::OpenAiCompatible { adapter, config } => {
                call_openai_prompt(adapter, config, request, is_cancelled)
            }
            Self::OpenAiResponses { adapter, config } => {
                call_openai_responses_prompt(adapter, config, request, is_cancelled)
            }
            Self::AnthropicMessages { adapter, config } => {
                call_anthropic_prompt(adapter, config, request, is_cancelled)
            }
            Self::AzureOpenAi { adapter, config } => {
                call_azure_openai_prompt(adapter, config, request, is_cancelled)
            }
        }
    }

    #[cfg(test)]
    fn contract_id(&self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::OpenAiCompatible { .. } => "openai_compatible",
            Self::OpenAiResponses { .. } => "openai_responses",
            Self::AnthropicMessages { .. } => "anthropic_messages",
            Self::AzureOpenAi { .. } => "azure_openai",
        }
    }
}

#[derive(Debug)]
struct ProviderJsonResponse {
    json: serde_json::Value,
    raw: String,
    attempts: u64,
    http_status: Option<u16>,
    retry_after_ms: Option<u64>,
    retry_delay_ms: Option<u64>,
    time_to_first_byte_ms: Option<f64>,
    time_to_first_token_ms: Option<f64>,
    request_total_ms: Option<f64>,
}

#[derive(Debug)]
struct ProviderHttpResponse {
    body: String,
    status: Option<u16>,
    retry_after_ms: Option<u64>,
    time_to_first_byte_ms: Option<f64>,
    request_total_ms: Option<f64>,
}

#[derive(Debug)]
struct ProviderStreamResponse {
    content: String,
    raw: String,
    metrics: serde_json::Map<String, serde_json::Value>,
    attempts: u64,
    http_status: Option<u16>,
    retry_after_ms: Option<u64>,
    retry_delay_ms: Option<u64>,
    time_to_first_byte_ms: Option<f64>,
    time_to_first_token_ms: Option<f64>,
    request_total_ms: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
enum StreamFormat {
    OpenAiChat,
    OpenAiResponses,
    AnthropicMessages,
}

struct PromptScore {
    status: String,
    score: f64,
    tests: serde_json::Value,
    error_message: Option<String>,
}

#[derive(Clone)]
struct ParallelWorkItem {
    target: store::TargetRecord,
    pack: Arc<BenchmarkPackSpec>,
    task: TaskSpec,
    docker: bool,
    warmup_runs: u32,
    concurrency: u32,
    run_group_id: Option<String>,
}

enum ParallelRunMessage {
    Started { target_id: String, task_id: String },
    Completed(RunResultDto),
    Failed(String),
}

const MAX_OUTPUT_BYTES: usize = 1_000_000;
const MAX_RUN_CONCURRENCY: u32 = 8;
const MAX_PROVIDER_RETRY_AFTER_MS: u64 = 30_000;
const SANDBOX_HOME_DIR: &str = ".benchforge-home";
const SANDBOX_TMP_DIR: &str = ".benchforge-tmp";
const SANDBOX_NPM_CACHE_DIR: &str = ".benchforge-npm-cache";
const DOCKER_SCORING_CPUS: &str = "2.0";
const DOCKER_SCORING_MEMORY: &str = "2g";
const DOCKER_SCORING_PIDS_LIMIT: &str = "256";
const DOCKER_SCORING_CAP_DROP: &str = "ALL";
const DOCKER_SCORING_SECURITY_OPT: &str = "no-new-privileges:true";
pub(crate) const CLOUD_CONTRACT_API_KEY_ENV: &str = "BENCHFORGE_CLOUD_CONTRACT_API_KEY";
const CLOUD_CONTRACT_EXPECTED_REPLY: &str = "benchforge-cloud-ok";

fn default_pack() -> String {
    "quick-smoke".into()
}

fn default_repetitions() -> u32 {
    1
}

fn default_concurrency() -> u32 {
    1
}

fn default_timeout() -> u64 {
    600
}

fn default_task_weight() -> f64 {
    1.0
}

fn normalized_task_weight(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

fn default_number_tolerance() -> f64 {
    0.000001
}

pub fn normalized_concurrency(value: u32) -> u32 {
    value.clamp(1, MAX_RUN_CONCURRENCY)
}

pub fn create_run_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn list_benchmark_packs() -> Result<Vec<BenchmarkPackDto>, String> {
    Ok(scan_benchmark_packs(&benchmark_pack_roots()).packs)
}

pub fn list_benchmark_pack_diagnostics() -> Vec<BenchmarkPackDiagnosticDto> {
    scan_benchmark_packs(&benchmark_pack_roots()).diagnostics
}

pub fn list_benchmark_pack_tasks(pack_id: String) -> Result<Vec<BenchmarkPackTaskDto>, String> {
    let pack_id = pack_id.trim();
    validate_benchmark_pack_id(pack_id)?;
    let pack = load_pack(pack_id)?;
    let tasks = load_tasks(&pack)?;
    Ok(tasks.iter().map(benchmark_pack_task_dto).collect())
}

pub fn create_benchmark_pack_template(
    request: CreateBenchmarkPackTemplateRequest,
) -> Result<CreatedBenchmarkPackTemplateDto, String> {
    create_benchmark_pack_template_in_root(&user_benchmark_pack_root(), request)
}

pub fn add_benchmark_pack_prompt_task(
    request: AddBenchmarkPackPromptTaskRequest,
) -> Result<AddedBenchmarkPackPromptTaskDto, String> {
    add_benchmark_pack_prompt_task_in_root(&user_benchmark_pack_root(), request)
}

pub fn update_benchmark_pack_prompt_task(
    request: UpdateBenchmarkPackPromptTaskRequest,
) -> Result<UpdatedBenchmarkPackPromptTaskDto, String> {
    update_benchmark_pack_prompt_task_in_root(&user_benchmark_pack_root(), request)
}

pub fn update_benchmark_pack_calibration(
    request: UpdateBenchmarkPackCalibrationRequest,
) -> Result<UpdatedBenchmarkPackCalibrationDto, String> {
    update_benchmark_pack_calibration_in_root(&user_benchmark_pack_root(), request)
}

pub fn score_prompt_task_preview(
    request: ScorePromptTaskPreviewRequest,
) -> Result<ScorePromptTaskPreviewDto, String> {
    let scoring = prompt_task_scoring_spec_from_request(&AddBenchmarkPackPromptTaskRequest {
        pack_id: "preview".into(),
        task_id: Some("preview".into()),
        name: "Preview".into(),
        prompt: "Preview".into(),
        scoring_method: request.scoring_method,
        expected_response: request.expected_response,
        timeout_seconds: Some(120),
        weight: None,
    })?;
    let score = score_prompt_response(&scoring, &request.sample_response);
    let task = TaskSpec {
        id: "preview".into(),
        name: "Preview".into(),
        task_type: "prompt".into(),
        version: None,
        language: None,
        fixture: None,
        prompt: "Preview".into(),
        timeout_seconds: 120,
        max_turns: None,
        weight: 1.0,
        scoring,
        source_path: PathBuf::new(),
    };
    Ok(ScorePromptTaskPreviewDto {
        status: score.status,
        score: score.score,
        tests: score.tests,
        error_message: score.error_message,
        scoring_methods: scoring_method_labels(&task),
    })
}

pub fn delete_benchmark_pack_task(
    request: DeleteBenchmarkPackTaskRequest,
) -> Result<DeletedBenchmarkPackTaskDto, String> {
    delete_benchmark_pack_task_in_root(&user_benchmark_pack_root(), request)
}

pub fn export_benchmark_pack(
    request: ExportBenchmarkPackRequest,
) -> Result<ExportedBenchmarkPackDto, String> {
    export_benchmark_pack_to_root(request, None)
}

pub fn import_benchmark_pack(
    request: ImportBenchmarkPackRequest,
) -> Result<ImportedBenchmarkPackDto, String> {
    import_benchmark_pack_into_root(
        &user_benchmark_pack_root(),
        &benchmark_pack_roots(),
        request,
    )
}

#[derive(Debug, Clone)]
struct BenchmarkPackRoot {
    path: PathBuf,
    source: &'static str,
    required: bool,
}

#[derive(Debug, Clone)]
struct DiscoveredBenchmarkPack {
    path: PathBuf,
    source: &'static str,
}

struct BenchmarkPackScan {
    packs: Vec<BenchmarkPackDto>,
    diagnostics: Vec<BenchmarkPackDiagnosticDto>,
}

struct ScannedValidPack {
    dto: BenchmarkPackDto,
    diagnostic: BenchmarkPackDiagnosticDto,
}

fn builtin_benchmark_pack_root() -> PathBuf {
    paths::resource_root().join("benchmark-packs")
}

fn user_benchmark_pack_root() -> PathBuf {
    paths::app_data_dir().join("benchmark-packs")
}

fn default_benchmark_pack_export_root() -> PathBuf {
    paths::app_data_dir()
        .join("exports")
        .join("benchmark-packs")
}

fn benchmark_pack_roots() -> Vec<BenchmarkPackRoot> {
    let mut roots = vec![BenchmarkPackRoot {
        path: builtin_benchmark_pack_root(),
        source: "built-in",
        required: true,
    }];
    if let Some(extra_roots) = std::env::var_os("BENCHFORGE_BENCHMARK_PACK_DIRS") {
        roots.extend(
            std::env::split_paths(&extra_roots).map(|path| BenchmarkPackRoot {
                path,
                source: "external",
                required: false,
            }),
        );
    }
    roots.push(BenchmarkPackRoot {
        path: user_benchmark_pack_root(),
        source: "user",
        required: false,
    });
    roots
}

fn discover_benchmark_pack_paths(
    roots: &[BenchmarkPackRoot],
) -> Result<Vec<DiscoveredBenchmarkPack>, String> {
    let mut paths = Vec::new();
    for root in roots {
        if !root.path.exists() {
            if root.required {
                return Err(format!(
                    "{}: benchmark pack root missing",
                    root.path.display()
                ));
            }
            continue;
        }
        let root_path = root
            .path
            .canonicalize()
            .map_err(|err| format!("{}: {}", root.path.display(), err))?;
        for entry in
            fs::read_dir(&root_path).map_err(|err| format!("{}: {}", root_path.display(), err))?
        {
            let entry = entry.map_err(|err| err.to_string())?;
            let pack_path = entry.path().join("pack.yaml");
            if pack_path.exists() {
                paths.push(DiscoveredBenchmarkPack {
                    path: pack_path,
                    source: root.source,
                });
            }
        }
    }
    Ok(paths)
}

fn scan_benchmark_packs(roots: &[BenchmarkPackRoot]) -> BenchmarkPackScan {
    let mut diagnostics = Vec::new();
    let mut valid_by_id: HashMap<String, Vec<ScannedValidPack>> = HashMap::new();
    let discovered = discover_benchmark_pack_paths_for_scan(roots, &mut diagnostics);
    for discovered in discovered {
        match load_pack_from_path_with_source(&discovered.path, discovered.source) {
            Ok(pack) => match load_tasks(&pack) {
                Ok(tasks) => {
                    let dto = benchmark_pack_dto(&pack, &tasks);
                    let diagnostic = BenchmarkPackDiagnosticDto {
                        id: Some(pack.id.clone()),
                        source: pack.source.clone(),
                        source_path: pack.pack_dir.to_string_lossy().to_string(),
                        status: "ok".into(),
                        detail: format!("{} task(s) loaded", tasks.len()),
                    };
                    valid_by_id
                        .entry(pack.id.clone())
                        .or_default()
                        .push(ScannedValidPack { dto, diagnostic });
                }
                Err(err) => diagnostics.push(BenchmarkPackDiagnosticDto {
                    id: Some(pack.id.clone()),
                    source: pack.source.clone(),
                    source_path: pack.pack_dir.to_string_lossy().to_string(),
                    status: "error".into(),
                    detail: err,
                }),
            },
            Err(err) => diagnostics.push(BenchmarkPackDiagnosticDto {
                id: None,
                source: discovered.source.into(),
                source_path: discovered
                    .path
                    .parent()
                    .unwrap_or(&discovered.path)
                    .to_string_lossy()
                    .to_string(),
                status: "error".into(),
                detail: err,
            }),
        }
    }

    let mut packs = Vec::new();
    for (id, scanned) in valid_by_id {
        if scanned.len() == 1 {
            let scanned = scanned.into_iter().next().expect("one scanned pack");
            diagnostics.push(scanned.diagnostic);
            packs.push(scanned.dto);
        } else {
            let locations = scanned
                .iter()
                .map(|entry| entry.dto.source_path.clone())
                .collect::<Vec<_>>()
                .join(", ");
            for entry in scanned {
                diagnostics.push(BenchmarkPackDiagnosticDto {
                    id: Some(id.clone()),
                    source: entry.dto.source.clone(),
                    source_path: entry.dto.source_path.clone(),
                    status: "error".into(),
                    detail: format!(
                        "duplicate benchmark pack id {}; remove or rename one of: {}",
                        id, locations
                    ),
                });
            }
        }
    }

    packs.sort_by(|a, b| a.id.cmp(&b.id));
    diagnostics.sort_by(|a, b| {
        a.status
            .cmp(&b.status)
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.source_path.cmp(&b.source_path))
    });
    BenchmarkPackScan { packs, diagnostics }
}

fn discover_benchmark_pack_paths_for_scan(
    roots: &[BenchmarkPackRoot],
    diagnostics: &mut Vec<BenchmarkPackDiagnosticDto>,
) -> Vec<DiscoveredBenchmarkPack> {
    let mut paths = Vec::new();
    for root in roots {
        if !root.path.exists() {
            if root.required {
                diagnostics.push(BenchmarkPackDiagnosticDto {
                    id: None,
                    source: root.source.into(),
                    source_path: root.path.to_string_lossy().to_string(),
                    status: "error".into(),
                    detail: "benchmark pack root missing".into(),
                });
            }
            continue;
        }
        let root_path = match root.path.canonicalize() {
            Ok(path) => path,
            Err(err) => {
                diagnostics.push(BenchmarkPackDiagnosticDto {
                    id: None,
                    source: root.source.into(),
                    source_path: root.path.to_string_lossy().to_string(),
                    status: "error".into(),
                    detail: err.to_string(),
                });
                continue;
            }
        };
        let entries = match fs::read_dir(&root_path) {
            Ok(entries) => entries,
            Err(err) => {
                diagnostics.push(BenchmarkPackDiagnosticDto {
                    id: None,
                    source: root.source.into(),
                    source_path: root_path.to_string_lossy().to_string(),
                    status: "error".into(),
                    detail: err.to_string(),
                });
                continue;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let pack_path = entry.path().join("pack.yaml");
                    if pack_path.exists() {
                        paths.push(DiscoveredBenchmarkPack {
                            path: pack_path,
                            source: root.source,
                        });
                    }
                }
                Err(err) => diagnostics.push(BenchmarkPackDiagnosticDto {
                    id: None,
                    source: root.source.into(),
                    source_path: root_path.to_string_lossy().to_string(),
                    status: "error".into(),
                    detail: err.to_string(),
                }),
            }
        }
    }
    paths
}

fn benchmark_pack_dto(pack: &BenchmarkPackSpec, tasks: &[TaskSpec]) -> BenchmarkPackDto {
    let heavy = pack.tags.iter().any(|tag| tag == "heavy")
        || pack
            .estimated_runtime
            .as_deref()
            .unwrap_or("")
            .contains("hours");
    let pack_summary = summarize_pack_metadata(pack, tasks);
    let evidence = benchmark_pack_evidence_profile(pack, tasks, &pack_summary.scoring_methods);
    let calibration = benchmark_pack_calibration(pack);
    BenchmarkPackDto {
        id: pack.id.clone(),
        name: pack.name.clone(),
        version: pack.version.clone(),
        description: pack.description.clone(),
        tags: pack.tags.clone(),
        estimated_runtime: pack.estimated_runtime.clone(),
        requires_sandbox: pack.requires_sandbox,
        tasks: tasks.len(),
        prompt_tasks: evidence.prompt_tasks,
        total_task_weight: evidence.total_task_weight,
        heavy,
        task_types: pack_summary.task_types,
        languages: pack_summary.languages,
        required_tools: pack_summary.required_tools,
        scoring_methods: pack_summary.scoring_methods,
        supported_target_kinds: pack_summary.supported_target_kinds,
        target_fit: pack_summary.target_fit,
        evidence_profile: evidence.profile,
        evidence_warnings: evidence.warnings,
        calibration_status: calibration.status,
        calibration_sample_size: calibration.sample_size,
        calibration_baseline_models: calibration.baseline_models,
        calibration_last_reviewed: calibration.last_reviewed,
        calibration_review_scope: calibration.review_scope,
        calibration_quality_gates: calibration.quality_gates,
        calibration_notes: calibration.notes,
        source: pack.source.clone(),
        source_path: pack.pack_dir.to_string_lossy().to_string(),
    }
}

fn benchmark_pack_reproducibility(pack: &BenchmarkPackSpec) -> Result<serde_json::Value, String> {
    let tasks = load_tasks(pack)?;
    let pack_summary = summarize_pack_metadata(pack, &tasks);
    let evidence = benchmark_pack_evidence_profile(pack, &tasks, &pack_summary.scoring_methods);
    let calibration = benchmark_pack_calibration(pack);
    Ok(serde_json::json!({
        "id": &pack.id,
        "version": &pack.version,
        "source": &pack.source,
        "checksum": checksum_file(&pack_file_path(pack))?,
        "evidence_profile": evidence.profile,
        "evidence_warnings": evidence.warnings,
        "prompt_tasks": evidence.prompt_tasks,
        "total_task_weight": evidence.total_task_weight,
        "calibration": {
            "status": calibration.status,
            "sample_size": calibration.sample_size,
            "baseline_models": calibration.baseline_models,
            "last_reviewed": calibration.last_reviewed,
            "review_scope": calibration.review_scope,
            "quality_gates": calibration.quality_gates,
            "notes": calibration.notes
        }
    }))
}

fn benchmark_pack_task_dto(task: &TaskSpec) -> BenchmarkPackTaskDto {
    BenchmarkPackTaskDto {
        id: task.id.clone(),
        name: task.name.clone(),
        task_type: task.task_type.clone(),
        language: task.language.clone(),
        fixture: task.fixture.clone(),
        prompt: task.prompt.clone(),
        timeout_seconds: task.timeout_seconds,
        max_turns: task.max_turns,
        weight: normalized_task_weight(task.weight),
        scoring_methods: scoring_method_labels(task),
        scoring: serde_json::to_value(&task.scoring).unwrap_or_else(|_| serde_json::json!({})),
        source_path: task.source_path.to_string_lossy().to_string(),
    }
}

#[derive(Serialize)]
struct BenchmarkPackTemplateYaml {
    id: String,
    name: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    tags: Vec<String>,
    estimated_runtime: String,
    requires_sandbox: bool,
    calibration: BenchmarkPackCalibrationSpec,
    tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EditableBenchmarkPackYaml {
    id: String,
    name: String,
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    estimated_runtime: Option<String>,
    #[serde(default)]
    requires_sandbox: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    calibration: Option<BenchmarkPackCalibrationSpec>,
    tasks: Vec<String>,
}

#[derive(Serialize)]
struct BenchmarkTaskTemplateYaml {
    id: String,
    name: String,
    version: String,
    #[serde(rename = "type")]
    task_type: String,
    prompt: String,
    timeout_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
    scoring: BenchmarkTaskPromptScoringTemplateYaml,
}

#[derive(Default, Serialize)]
struct BenchmarkTaskPromptScoringTemplateYaml {
    #[serde(skip_serializing_if = "Option::is_none")]
    expect_exact: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    expect_contains: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    expect_regex: Vec<String>,
    #[serde(skip_serializing_if = "is_false")]
    expect_json: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    json_field_equals: BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    json_field_contains: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    json_field_object_keys_exact: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    json_field_array_exact: BTreeMap<String, Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    json_field_array_exact_ordered: BTreeMap<String, Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    json_field_number_close: BTreeMap<String, JsonNumberCloseSpec>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    json_field_number_bounds: BTreeMap<String, JsonNumberBoundsSpec>,
}

fn create_benchmark_pack_template_in_root(
    root: &Path,
    request: CreateBenchmarkPackTemplateRequest,
) -> Result<CreatedBenchmarkPackTemplateDto, String> {
    let id = request.id.trim();
    validate_benchmark_pack_id(id)?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err("benchmark pack name is required".into());
    }
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err("benchmark pack template prompt is required".into());
    }
    let expected_response = request.expected_response.trim();
    if expected_response.is_empty() {
        return Err("benchmark pack template expected response is required".into());
    }

    fs::create_dir_all(root).map_err(|err| format!("{}: {}", root.display(), err))?;
    let pack_dir = root.join(id);
    if pack_dir.exists() {
        return Err(format!(
            "benchmark pack {} already exists at {}",
            id,
            pack_dir.display()
        ));
    }

    let task_id = format!("{}-prompt-001", id);
    validate_benchmark_pack_id(&task_id)?;
    let task_relative_path = format!("tasks/{}.yaml", task_id);
    let tasks_dir = pack_dir.join("tasks");
    fs::create_dir_all(&tasks_dir).map_err(|err| format!("{}: {}", tasks_dir.display(), err))?;

    let pack_yaml = BenchmarkPackTemplateYaml {
        id: id.to_string(),
        name: name.to_string(),
        version: "0.1.0".into(),
        description: request
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        tags: vec!["prompt".into(), "llm".into(), "private".into()],
        estimated_runtime: "1-3 minutes".into(),
        requires_sandbox: false,
        calibration: BenchmarkPackCalibrationSpec {
            status: "uncalibrated".into(),
            sample_size: None,
            baseline_models: Vec::new(),
            last_reviewed: None,
            review_scope: Some("none".into()),
            quality_gates: vec!["review_before_public_leaderboard".into()],
            notes: Some(
                "Starter private pack; add tasks, run pilot comparisons, and record calibration before using as definitive evidence."
                    .into(),
            ),
        },
        tasks: vec![task_relative_path.clone()],
    };
    let task_yaml = BenchmarkTaskTemplateYaml {
        id: task_id.clone(),
        name: "Private prompt check".into(),
        version: "0.1.0".into(),
        task_type: "prompt".into(),
        prompt: prompt.to_string(),
        timeout_seconds: 120,
        weight: None,
        scoring: BenchmarkTaskPromptScoringTemplateYaml {
            expect_exact: Some(expected_response.to_string()),
            ..BenchmarkTaskPromptScoringTemplateYaml::default()
        },
    };

    let pack_path = pack_dir.join("pack.yaml");
    let task_path = pack_dir.join(&task_relative_path);
    fs::write(
        &pack_path,
        serde_yaml::to_string(&pack_yaml).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    fs::write(
        &task_path,
        serde_yaml::to_string(&task_yaml).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("{}: {}", task_path.display(), err))?;

    let pack = load_pack_from_path_with_source(&pack_path, "user")?;
    let tasks = load_tasks(&pack)?;
    let dto = benchmark_pack_dto(&pack, &tasks);
    Ok(CreatedBenchmarkPackTemplateDto {
        pack: dto,
        source_path: pack.pack_dir.to_string_lossy().to_string(),
        task_path: task_path.to_string_lossy().to_string(),
    })
}

fn add_benchmark_pack_prompt_task_in_root(
    root: &Path,
    request: AddBenchmarkPackPromptTaskRequest,
) -> Result<AddedBenchmarkPackPromptTaskDto, String> {
    let pack_id = request.pack_id.trim();
    validate_benchmark_pack_id(pack_id)?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err("benchmark task name is required".into());
    }
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err("benchmark task prompt is required".into());
    }
    let timeout_seconds = request.timeout_seconds.unwrap_or(120);
    if !(1..=3600).contains(&timeout_seconds) {
        return Err("benchmark task timeout must be between 1 and 3600 seconds".into());
    }
    if request
        .weight
        .is_some_and(|weight| !weight.is_finite() || weight <= 0.0 || weight > 100.0)
    {
        return Err("benchmark task weight must be greater than 0 and at most 100".into());
    }

    let pack_dir = root.join(pack_id);
    let pack_path = pack_dir.join("pack.yaml");
    if !pack_path.exists() {
        return Err(format!(
            "user benchmark pack {} was not found at {}",
            pack_id,
            pack_path.display()
        ));
    }
    let pack = load_pack_from_path_with_source(&pack_path, "user")?;
    if pack.id != pack_id {
        return Err(format!(
            "benchmark pack id mismatch: requested {}, found {}",
            pack_id, pack.id
        ));
    }
    let existing_tasks = load_tasks(&pack)?;
    let existing_task_ids: BTreeSet<String> =
        existing_tasks.iter().map(|task| task.id.clone()).collect();
    let task_id = request
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| next_prompt_task_id(pack_id, name, &existing_task_ids));
    validate_benchmark_pack_id(&task_id)?;
    if existing_task_ids.contains(&task_id) {
        return Err(format!(
            "benchmark task id {} already exists in pack {}",
            task_id, pack_id
        ));
    }

    let scoring = prompt_task_scoring_from_request(&request)?;
    let task_relative_path = format!("tasks/{}.yaml", task_id);
    if pack.tasks.iter().any(|path| path == &task_relative_path) {
        return Err(format!(
            "benchmark task path {} already exists in pack {}",
            task_relative_path, pack_id
        ));
    }
    let tasks_dir = pack_dir.join("tasks");
    fs::create_dir_all(&tasks_dir).map_err(|err| format!("{}: {}", tasks_dir.display(), err))?;
    let task_path = pack_dir.join(&task_relative_path);
    if task_path.exists() {
        return Err(format!(
            "benchmark task file already exists at {}",
            task_path.display()
        ));
    }

    let task_yaml = BenchmarkTaskTemplateYaml {
        id: task_id.clone(),
        name: name.to_string(),
        version: "0.1.0".into(),
        task_type: "prompt".into(),
        prompt: prompt.to_string(),
        timeout_seconds,
        weight: request.weight,
        scoring,
    };

    let raw_pack = fs::read_to_string(&pack_path)
        .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    let mut editable: EditableBenchmarkPackYaml = serde_yaml::from_str(&raw_pack)
        .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    if editable.id != pack_id {
        return Err(format!(
            "benchmark pack id mismatch: requested {}, found {}",
            pack_id, editable.id
        ));
    }
    editable.tasks.push(task_relative_path);

    fs::write(
        &task_path,
        serde_yaml::to_string(&task_yaml).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("{}: {}", task_path.display(), err))?;
    fs::write(
        &pack_path,
        serde_yaml::to_string(&editable).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("{}: {}", pack_path.display(), err))?;

    let updated_pack = load_pack_from_path_with_source(&pack_path, "user")?;
    let updated_tasks = load_tasks(&updated_pack)?;
    let dto = benchmark_pack_dto(&updated_pack, &updated_tasks);
    Ok(AddedBenchmarkPackPromptTaskDto {
        pack: dto,
        source_path: updated_pack.pack_dir.to_string_lossy().to_string(),
        task_path: task_path.to_string_lossy().to_string(),
        task_id,
    })
}

fn update_benchmark_pack_prompt_task_in_root(
    root: &Path,
    request: UpdateBenchmarkPackPromptTaskRequest,
) -> Result<UpdatedBenchmarkPackPromptTaskDto, String> {
    let pack_id = request.pack_id.trim();
    let task_id = request.task_id.trim();
    validate_benchmark_pack_id(pack_id)?;
    validate_benchmark_pack_id(task_id)?;
    let name = request.name.trim();
    if name.is_empty() {
        return Err("benchmark task name is required".into());
    }
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err("benchmark task prompt is required".into());
    }
    let timeout_seconds = request.timeout_seconds.unwrap_or(120);
    if !(1..=3600).contains(&timeout_seconds) {
        return Err("benchmark task timeout must be between 1 and 3600 seconds".into());
    }
    if request
        .weight
        .is_some_and(|weight| !weight.is_finite() || weight <= 0.0 || weight > 100.0)
    {
        return Err("benchmark task weight must be greater than 0 and at most 100".into());
    }

    let pack_dir = root.join(pack_id);
    let pack_path = pack_dir.join("pack.yaml");
    if !pack_path.exists() {
        return Err(format!(
            "user benchmark pack {} was not found at {}",
            pack_id,
            pack_path.display()
        ));
    }
    let pack = load_pack_from_path_with_source(&pack_path, "user")?;
    if pack.id != pack_id {
        return Err(format!(
            "benchmark pack id mismatch: requested {}, found {}",
            pack_id, pack.id
        ));
    }
    let tasks = load_tasks(&pack)?;
    let task = tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| {
            format!(
                "benchmark task {} was not found in pack {}",
                task_id, pack_id
            )
        })?;
    if task.task_type != "prompt" {
        return Err(format!(
            "benchmark task {} is type {}; only prompt tasks can be edited here",
            task_id, task.task_type
        ));
    }
    let task_path = task.source_path.clone();
    let scoring = prompt_task_scoring_from_request(&AddBenchmarkPackPromptTaskRequest {
        pack_id: pack_id.to_string(),
        task_id: Some(task_id.to_string()),
        name: name.to_string(),
        prompt: prompt.to_string(),
        scoring_method: request.scoring_method.clone(),
        expected_response: request.expected_response.clone(),
        timeout_seconds: Some(timeout_seconds),
        weight: request.weight,
    })?;
    let task_yaml = BenchmarkTaskTemplateYaml {
        id: task_id.to_string(),
        name: name.to_string(),
        version: task.version.clone().unwrap_or_else(|| "0.1.0".into()),
        task_type: "prompt".into(),
        prompt: prompt.to_string(),
        timeout_seconds,
        weight: request.weight,
        scoring,
    };
    fs::write(
        &task_path,
        serde_yaml::to_string(&task_yaml).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("{}: {}", task_path.display(), err))?;

    let updated_pack = load_pack_from_path_with_source(&pack_path, "user")?;
    let updated_tasks = load_tasks(&updated_pack)?;
    let dto = benchmark_pack_dto(&updated_pack, &updated_tasks);
    Ok(UpdatedBenchmarkPackPromptTaskDto {
        pack: dto,
        source_path: updated_pack.pack_dir.to_string_lossy().to_string(),
        task_path: task_path.to_string_lossy().to_string(),
        task_id: task_id.to_string(),
    })
}

fn update_benchmark_pack_calibration_in_root(
    root: &Path,
    request: UpdateBenchmarkPackCalibrationRequest,
) -> Result<UpdatedBenchmarkPackCalibrationDto, String> {
    let pack_id = request.pack_id.trim();
    validate_benchmark_pack_id(pack_id)?;
    let status = normalized_calibration_status(&request.status);
    if !matches!(
        status.as_str(),
        "uncalibrated" | "pilot" | "reviewed" | "calibrated"
    ) {
        return Err(
            "benchmark pack calibration status must be uncalibrated, pilot, reviewed, or calibrated"
                .into(),
        );
    }
    let baseline_models = unique_strings(
        request
            .baseline_models
            .iter()
            .map(|model| model.trim().to_string())
            .filter(|model| !model.is_empty()),
    );
    if baseline_models.iter().any(|model| model.len() > 200) {
        return Err(
            "benchmark pack calibration baseline model names must be 200 characters or fewer"
                .into(),
        );
    }
    if baseline_models.len() > 50 {
        return Err("benchmark pack calibration can list at most 50 baseline models".into());
    }
    let last_reviewed = trimmed_optional_string(request.last_reviewed.as_deref());
    if let Some(value) = &last_reviewed {
        validate_calibration_review_date(value)?;
    }
    let notes = trimmed_optional_string(request.notes.as_deref());
    if notes.as_ref().is_some_and(|value| value.len() > 2000) {
        return Err("benchmark pack calibration notes must be 2000 characters or fewer".into());
    }
    if status == "calibrated" {
        if request.sample_size.unwrap_or(0) == 0 {
            return Err("calibrated benchmark packs must record a positive sample size".into());
        }
        if baseline_models.len() < 2 {
            return Err("calibrated benchmark packs must list at least two baseline models".into());
        }
        if last_reviewed.is_none() {
            return Err("calibrated benchmark packs must record a last reviewed date".into());
        }
        if notes.is_none() {
            return Err("calibrated benchmark packs must include review notes".into());
        }
    }

    let pack_dir = root.join(pack_id);
    let pack_path = pack_dir.join("pack.yaml");
    if !pack_path.exists() {
        return Err(format!(
            "user benchmark pack {} was not found at {}",
            pack_id,
            pack_path.display()
        ));
    }
    let raw_pack = fs::read_to_string(&pack_path)
        .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    let mut editable: EditableBenchmarkPackYaml = serde_yaml::from_str(&raw_pack)
        .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    if editable.id != pack_id {
        return Err(format!(
            "benchmark pack id mismatch: requested {}, found {}",
            pack_id, editable.id
        ));
    }

    let review_scope = if status == "uncalibrated" {
        "none".into()
    } else {
        "pilot_runs".into()
    };
    editable.calibration = Some(BenchmarkPackCalibrationSpec {
        status,
        sample_size: request.sample_size,
        baseline_models,
        last_reviewed,
        review_scope: Some(review_scope),
        quality_gates: default_user_calibration_quality_gates(),
        notes,
    });
    fs::write(
        &pack_path,
        serde_yaml::to_string(&editable).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("{}: {}", pack_path.display(), err))?;

    let updated_pack = load_pack_from_path_with_source(&pack_path, "user")?;
    let updated_tasks = load_tasks(&updated_pack)?;
    let dto = benchmark_pack_dto(&updated_pack, &updated_tasks);
    Ok(UpdatedBenchmarkPackCalibrationDto {
        pack: dto,
        source_path: updated_pack.pack_dir.to_string_lossy().to_string(),
    })
}

fn delete_benchmark_pack_task_in_root(
    root: &Path,
    request: DeleteBenchmarkPackTaskRequest,
) -> Result<DeletedBenchmarkPackTaskDto, String> {
    let pack_id = request.pack_id.trim();
    let task_id = request.task_id.trim();
    validate_benchmark_pack_id(pack_id)?;
    validate_benchmark_pack_id(task_id)?;

    let pack_dir = root.join(pack_id);
    let pack_path = pack_dir.join("pack.yaml");
    if !pack_path.exists() {
        return Err(format!(
            "user benchmark pack {} was not found at {}",
            pack_id,
            pack_path.display()
        ));
    }
    let pack = load_pack_from_path_with_source(&pack_path, "user")?;
    if pack.id != pack_id {
        return Err(format!(
            "benchmark pack id mismatch: requested {}, found {}",
            pack_id, pack.id
        ));
    }
    if pack.tasks.len() <= 1 {
        return Err(format!(
            "benchmark pack {} must keep at least one task",
            pack_id
        ));
    }

    let tasks = load_tasks(&pack)?;
    let task_index = tasks
        .iter()
        .position(|task| task.id == task_id)
        .ok_or_else(|| {
            format!(
                "benchmark task {} was not found in pack {}",
                task_id, pack_id
            )
        })?;
    let deleted_relative_path = pack
        .tasks
        .get(task_index)
        .ok_or_else(|| format!("benchmark task {} has no pack task path", task_id))?
        .clone();
    let deleted_source_path = tasks[task_index].source_path.clone();

    let raw_pack = fs::read_to_string(&pack_path)
        .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    let mut editable: EditableBenchmarkPackYaml = serde_yaml::from_str(&raw_pack)
        .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    if editable.id != pack_id {
        return Err(format!(
            "benchmark pack id mismatch: requested {}, found {}",
            pack_id, editable.id
        ));
    }
    let before = editable.tasks.len();
    editable.tasks.retain(|path| path != &deleted_relative_path);
    if editable.tasks.len() == before {
        return Err(format!(
            "benchmark task path {} was not found in pack {}",
            deleted_relative_path, pack_id
        ));
    }
    if editable.tasks.is_empty() {
        return Err(format!(
            "benchmark pack {} must keep at least one task",
            pack_id
        ));
    }

    fs::write(
        &pack_path,
        serde_yaml::to_string(&editable).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("{}: {}", pack_path.display(), err))?;
    let _ = fs::remove_file(&deleted_source_path);

    let updated_pack = load_pack_from_path_with_source(&pack_path, "user")?;
    let updated_tasks = load_tasks(&updated_pack)?;
    let dto = benchmark_pack_dto(&updated_pack, &updated_tasks);
    Ok(DeletedBenchmarkPackTaskDto {
        pack: dto,
        source_path: updated_pack.pack_dir.to_string_lossy().to_string(),
        deleted_task_id: task_id.to_string(),
        deleted_task_path: deleted_source_path.to_string_lossy().to_string(),
    })
}

fn export_benchmark_pack_to_root(
    request: ExportBenchmarkPackRequest,
    default_export_root: Option<&Path>,
) -> Result<ExportedBenchmarkPackDto, String> {
    let pack_id = request.pack_id.trim();
    validate_benchmark_pack_id(pack_id)?;
    let export_format = normalized_benchmark_pack_export_format(request.format.as_deref())?;
    let pack = load_pack(pack_id)?;
    let tasks = load_tasks(&pack)?;
    let dto = benchmark_pack_dto(&pack, &tasks);
    let export_root = request
        .destination_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| default_export_root.map(Path::to_path_buf))
        .unwrap_or_else(default_benchmark_pack_export_root);
    fs::create_dir_all(&export_root)
        .map_err(|err| format!("{}: {}", export_root.display(), err))?;
    let export_name = format!(
        "{}-{}-{}",
        pack.id,
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        uuid::Uuid::new_v4().simple()
    );
    let export_path = if export_format == "zip" {
        export_root.join(format!("{export_name}.zip"))
    } else {
        export_root.join(export_name)
    };
    if export_path.exists() {
        return Err(format!(
            "benchmark pack export destination already exists: {}",
            export_path.display()
        ));
    }
    let files_copied = if export_format == "zip" {
        create_benchmark_pack_zip(&pack.pack_dir, &export_path).inspect_err(|_| {
            let _ = fs::remove_file(&export_path);
        })?
    } else {
        copy_benchmark_pack_directory(&pack.pack_dir, &export_path).inspect_err(|_| {
            let _ = fs::remove_dir_all(&export_path);
        })?
    };
    let verification_dir = ScopedTempDir::new("benchforge-pack-export-verify")?;
    let exported_pack_path = if export_format == "zip" {
        extract_benchmark_pack_zip(&export_path, verification_dir.path())?;
        find_extracted_benchmark_pack_path(verification_dir.path())?
    } else {
        export_path.join("pack.yaml")
    };
    let exported_pack = load_pack_from_path_with_source(&exported_pack_path, "export")?;
    let exported_tasks = load_tasks(&exported_pack)?;
    if exported_pack.id != pack.id || exported_tasks.len() != tasks.len() {
        if export_format == "zip" {
            let _ = fs::remove_file(&export_path);
        } else {
            let _ = fs::remove_dir_all(&export_path);
        }
        return Err(format!(
            "benchmark pack export verification failed for {}",
            pack.id
        ));
    }
    Ok(ExportedBenchmarkPackDto {
        pack: dto,
        source_path: pack.pack_dir.to_string_lossy().to_string(),
        export_path: export_path.to_string_lossy().to_string(),
        format: export_format.to_string(),
        files_copied,
    })
}

fn import_benchmark_pack_into_root(
    root: &Path,
    existing_roots: &[BenchmarkPackRoot],
    request: ImportBenchmarkPackRequest,
) -> Result<ImportedBenchmarkPackDto, String> {
    let source_input = request.source_path.trim();
    if source_input.is_empty() {
        return Err("benchmark pack import source path is required".into());
    }
    let normalized_source = normalize_import_pack_source(Path::new(source_input))?;
    let source_pack = load_pack_from_path_with_source(&normalized_source.pack_path, "import")?;
    let source_tasks = load_tasks(&source_pack)?;
    let existing = scan_benchmark_packs(existing_roots);
    if let Some(duplicate) = existing.packs.iter().find(|pack| pack.id == source_pack.id) {
        return Err(format!(
            "benchmark pack {} already exists at {}",
            source_pack.id, duplicate.source_path
        ));
    }
    fs::create_dir_all(root).map_err(|err| format!("{}: {}", root.display(), err))?;
    let destination_dir = root.join(&source_pack.id);
    if destination_dir.exists() {
        return Err(format!(
            "benchmark pack destination already exists: {}",
            destination_dir.display()
        ));
    }
    let files_copied = copy_benchmark_pack_directory(&source_pack.pack_dir, &destination_dir)
        .inspect_err(|_| {
            let _ = fs::remove_dir_all(&destination_dir);
        })?;
    let imported_pack_path = destination_dir.join("pack.yaml");
    let imported_pack = load_pack_from_path_with_source(&imported_pack_path, "user")?;
    let imported_tasks = load_tasks(&imported_pack)?;
    if imported_pack.id != source_pack.id || imported_tasks.len() != source_tasks.len() {
        let _ = fs::remove_dir_all(&destination_dir);
        return Err(format!(
            "benchmark pack import verification failed for {}",
            source_pack.id
        ));
    }
    let dto = benchmark_pack_dto(&imported_pack, &imported_tasks);
    Ok(ImportedBenchmarkPackDto {
        pack: dto,
        source_path: normalized_source.source_path.to_string_lossy().to_string(),
        import_path: imported_pack.pack_dir.to_string_lossy().to_string(),
        files_copied,
    })
}

struct NormalizedImportPackSource {
    pack_path: PathBuf,
    source_path: PathBuf,
    _temp_dir: Option<ScopedTempDir>,
}

fn normalize_import_pack_source(source: &Path) -> Result<NormalizedImportPackSource, String> {
    let metadata = fs::metadata(source).map_err(|err| format!("{}: {}", source.display(), err))?;
    if metadata.is_file() && path_has_extension(source, "zip") {
        let source_path = source
            .canonicalize()
            .map_err(|err| format!("{}: {}", source.display(), err))?;
        let temp_dir = ScopedTempDir::new("benchforge-pack-import-zip")?;
        extract_benchmark_pack_zip(&source_path, temp_dir.path())?;
        let pack_path = find_extracted_benchmark_pack_path(temp_dir.path())?;
        return Ok(NormalizedImportPackSource {
            pack_path,
            source_path,
            _temp_dir: Some(temp_dir),
        });
    }
    let pack_path = normalize_import_pack_path(source)?;
    let source_path = if metadata.is_dir() {
        source
            .canonicalize()
            .map_err(|err| format!("{}: {}", source.display(), err))?
    } else {
        pack_path.clone()
    };
    Ok(NormalizedImportPackSource {
        pack_path,
        source_path,
        _temp_dir: None,
    })
}

fn normalize_import_pack_path(source: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(source).map_err(|err| format!("{}: {}", source.display(), err))?;
    let pack_path = if metadata.is_dir() {
        source.join("pack.yaml")
    } else {
        if source.file_name().and_then(|name| name.to_str()) != Some("pack.yaml") {
            return Err(format!(
                "benchmark pack import source must be a pack folder, pack.yaml, or .zip archive: {}",
                source.display()
            ));
        }
        source.to_path_buf()
    };
    if !pack_path.exists() {
        return Err(format!("{} not found", pack_path.display()));
    }
    pack_path
        .canonicalize()
        .map_err(|err| format!("{}: {}", pack_path.display(), err))
}

fn normalized_benchmark_pack_export_format(format: Option<&str>) -> Result<&'static str, String> {
    match format
        .unwrap_or("folder")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "folder" | "directory" | "dir" => Ok("folder"),
        "zip" | "archive" => Ok("zip"),
        other => Err(format!(
            "unsupported benchmark pack export format: {other}; expected folder or zip"
        )),
    }
}

fn path_has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(extension))
        .unwrap_or(false)
}

struct ScopedTempDir {
    path: PathBuf,
}

impl ScopedTempDir {
    fn new(prefix: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).map_err(|err| format!("{}: {}", path.display(), err))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScopedTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_benchmark_pack_zip(source: &Path, destination: &Path) -> Result<usize, String> {
    let source_root = source
        .canonicalize()
        .map_err(|err| format!("{}: {}", source.display(), err))?;
    if !source_root.is_dir() {
        return Err(format!(
            "benchmark pack source is not a directory: {}",
            source_root.display()
        ));
    }
    if destination.exists() {
        return Err(format!(
            "benchmark pack zip destination already exists: {}",
            destination.display()
        ));
    }
    if destination_would_be_inside_source(&source_root, destination)? {
        return Err(format!(
            "benchmark pack zip destination must not be inside source pack: {}",
            destination.display()
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {}", parent.display(), err))?;
    }
    let file = fs::File::create(destination)
        .map_err(|err| format!("{}: {}", destination.display(), err))?;
    let mut zip = ZipWriter::new(file);
    let mut files_written = 0;
    write_benchmark_pack_zip_entries(&source_root, &source_root, &mut zip, &mut files_written)?;
    zip.finish()
        .map_err(|err| format!("{}: {}", destination.display(), err))?;
    Ok(files_written)
}

fn write_benchmark_pack_zip_entries(
    source_root: &Path,
    source: &Path,
    zip: &mut ZipWriter<fs::File>,
    files_written: &mut usize,
) -> Result<(), String> {
    let source = source
        .canonicalize()
        .map_err(|err| format!("{}: {}", source.display(), err))?;
    if !source.starts_with(source_root) {
        return Err(format!(
            "benchmark pack zip attempted to leave source root: {}",
            source.display()
        ));
    }
    for entry in fs::read_dir(&source).map_err(|err| format!("{}: {}", source.display(), err))? {
        let entry = entry.map_err(|err| err.to_string())?;
        if entry.file_name() == ".git" {
            continue;
        }
        let source_path = entry.path();
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|err| format!("{}: {}", source_path.display(), err))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "benchmark pack symlinks are not allowed: {}",
                source_path.display()
            ));
        }
        if metadata.is_dir() {
            write_benchmark_pack_zip_entries(source_root, &source_path, zip, files_written)?;
        } else if metadata.is_file() {
            let relative = source_path
                .strip_prefix(source_root)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
            let entry_name = zip_entry_name(relative)?;
            let options = FileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .unix_permissions(0o644);
            zip.start_file(entry_name, options)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
            let mut input = fs::File::open(&source_path)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
            std::io::copy(&mut input, zip)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
            *files_written += 1;
        } else {
            return Err(format!(
                "benchmark pack entry is not a regular file or directory: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

fn zip_entry_name(relative: &Path) -> Result<String, String> {
    let name = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if name.trim().is_empty() || name.starts_with('/') || name.contains("../") || name == ".." {
        return Err(format!(
            "unsafe benchmark pack zip entry name: {}",
            relative.display()
        ));
    }
    Ok(name)
}

fn extract_benchmark_pack_zip(zip_path: &Path, destination: &Path) -> Result<usize, String> {
    fs::create_dir_all(destination).map_err(|err| format!("{}: {}", destination.display(), err))?;
    let file =
        fs::File::open(zip_path).map_err(|err| format!("{}: {}", zip_path.display(), err))?;
    let mut archive =
        ZipArchive::new(file).map_err(|err| format!("{}: {}", zip_path.display(), err))?;
    let destination = destination
        .canonicalize()
        .map_err(|err| format!("{}: {}", destination.display(), err))?;
    let mut files_extracted = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("{} entry {}: {}", zip_path.display(), index, err))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("benchmark pack zip entry has unsafe path: {}", entry.name()))?;
        let relative = enclosed.to_path_buf();
        if zip_entry_should_skip(&relative) {
            continue;
        }
        if entry
            .unix_mode()
            .map(zip_unix_mode_is_symlink)
            .unwrap_or(false)
        {
            return Err(format!(
                "benchmark pack zip symlinks are not allowed: {}",
                entry.name()
            ));
        }
        let output_path = destination.join(&relative);
        if !output_path.starts_with(&destination) {
            return Err(format!(
                "benchmark pack zip entry tried to leave extraction root: {}",
                entry.name()
            ));
        }
        if entry.is_dir() || entry.name().ends_with('/') {
            fs::create_dir_all(&output_path)
                .map_err(|err| format!("{}: {}", output_path.display(), err))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {}", parent.display(), err))?;
        }
        let mut output = fs::File::create(&output_path)
            .map_err(|err| format!("{}: {}", output_path.display(), err))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|err| format!("{}: {}", output_path.display(), err))?;
        files_extracted += 1;
    }
    Ok(files_extracted)
}

fn zip_unix_mode_is_symlink(mode: u32) -> bool {
    mode & 0o170000 == 0o120000
}

fn zip_entry_should_skip(relative: &Path) -> bool {
    let first = relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .unwrap_or("");
    first == "__MACOSX"
        || relative
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == ".DS_Store")
            .unwrap_or(false)
}

fn find_extracted_benchmark_pack_path(root: &Path) -> Result<PathBuf, String> {
    let root_pack = root.join("pack.yaml");
    if root_pack.exists() {
        return root_pack
            .canonicalize()
            .map_err(|err| format!("{}: {}", root_pack.display(), err));
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(root).map_err(|err| format!("{}: {}", root.display(), err))? {
        let entry = entry.map_err(|err| err.to_string())?;
        if entry.file_name() == "__MACOSX" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() && path.join("pack.yaml").exists() {
            candidates.push(path.join("pack.yaml"));
        }
    }
    match candidates.len() {
        1 => candidates
            .remove(0)
            .canonicalize()
            .map_err(|err| format!("extracted benchmark pack: {}", err)),
        0 => Err("benchmark pack zip must contain pack.yaml at the archive root or in one top-level pack folder".into()),
        _ => Err("benchmark pack zip contains multiple top-level pack.yaml files".into()),
    }
}

fn copy_benchmark_pack_directory(source: &Path, destination: &Path) -> Result<usize, String> {
    let source_root = source
        .canonicalize()
        .map_err(|err| format!("{}: {}", source.display(), err))?;
    if !source_root.is_dir() {
        return Err(format!(
            "benchmark pack source is not a directory: {}",
            source_root.display()
        ));
    }
    if destination.exists() {
        return Err(format!(
            "benchmark pack destination already exists: {}",
            destination.display()
        ));
    }
    if destination_would_be_inside_source(&source_root, destination)? {
        return Err(format!(
            "benchmark pack destination must not be inside source pack: {}",
            destination.display()
        ));
    }
    copy_benchmark_pack_directory_checked(&source_root, &source_root, destination)
}

fn destination_would_be_inside_source(
    source_root: &Path,
    destination: &Path,
) -> Result<bool, String> {
    let parent = destination.parent().ok_or_else(|| {
        format!(
            "benchmark pack destination has no parent: {}",
            destination.display()
        )
    })?;
    if !parent.exists() {
        return Ok(false);
    }
    let parent = parent
        .canonicalize()
        .map_err(|err| format!("{}: {}", parent.display(), err))?;
    let destination_name = destination.file_name().ok_or_else(|| {
        format!(
            "benchmark pack destination has no file name: {}",
            destination.display()
        )
    })?;
    Ok(parent.join(destination_name).starts_with(source_root))
}

fn copy_benchmark_pack_directory_checked(
    source_root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<usize, String> {
    let source = source
        .canonicalize()
        .map_err(|err| format!("{}: {}", source.display(), err))?;
    if !source.starts_with(source_root) {
        return Err(format!(
            "benchmark pack copy attempted to leave source root: {}",
            source.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|err| format!("{}: {}", destination.display(), err))?;
    let mut files_copied = 0;
    for entry in fs::read_dir(&source).map_err(|err| format!("{}: {}", source.display(), err))? {
        let entry = entry.map_err(|err| err.to_string())?;
        if entry.file_name() == ".git" {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|err| format!("{}: {}", source_path.display(), err))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "benchmark pack symlinks are not allowed: {}",
                source_path.display()
            ));
        }
        if metadata.is_dir() {
            files_copied += copy_benchmark_pack_directory_checked(
                source_root,
                &source_path,
                &destination_path,
            )?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
            files_copied += 1;
        } else {
            return Err(format!(
                "benchmark pack entry is not a regular file or directory: {}",
                source_path.display()
            ));
        }
    }
    Ok(files_copied)
}

fn prompt_task_scoring_from_request(
    request: &AddBenchmarkPackPromptTaskRequest,
) -> Result<BenchmarkTaskPromptScoringTemplateYaml, String> {
    let expected = request.expected_response.as_deref().unwrap_or("").trim();
    match request.scoring_method.trim().to_ascii_lowercase().as_str() {
        "exact" => {
            if expected.is_empty() {
                return Err("exact scoring requires an expected response".into());
            }
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_exact: Some(expected.to_string()),
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "contains" => {
            let values = split_expected_lines(expected);
            if values.is_empty() {
                return Err("contains scoring requires at least one expected substring".into());
            }
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_contains: values,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "regex" => {
            let values = split_expected_lines(expected);
            if values.is_empty() {
                return Err("regex scoring requires at least one expected pattern".into());
            }
            for pattern in &values {
                Regex::new(pattern).map_err(|err| {
                    format!("invalid regex pattern {}: {}", pattern, err)
                })?;
            }
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_regex: values,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "json" => Ok(BenchmarkTaskPromptScoringTemplateYaml {
            expect_json: true,
            ..BenchmarkTaskPromptScoringTemplateYaml::default()
        }),
        "json_field_equals" | "json_fields" | "json_equals" => {
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_json: true,
                json_field_equals: expected_json_field_values(expected, "json_field_equals")?,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "json_field_contains" | "json_contains" => Ok(BenchmarkTaskPromptScoringTemplateYaml {
            expect_json: true,
            json_field_contains: expected_json_string_lists(expected, "json_field_contains")?,
            ..BenchmarkTaskPromptScoringTemplateYaml::default()
        }),
        "json_field_array_exact" | "json_array_exact" => {
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_json: true,
                json_field_array_exact: expected_json_value_arrays(
                    expected,
                    "json_field_array_exact",
                )?,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "json_field_array_exact_ordered" | "json_array_ordered" => {
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_json: true,
                json_field_array_exact_ordered: expected_json_value_arrays(
                    expected,
                    "json_field_array_exact_ordered",
                )?,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "json_field_object_keys_exact" | "json_object_keys" => {
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_json: true,
                json_field_object_keys_exact: expected_json_string_lists(
                    expected,
                    "json_field_object_keys_exact",
                )?,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "json_field_number_close" | "json_number_close" => {
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_json: true,
                json_field_number_close: expected_json_number_close(
                    expected,
                    "json_field_number_close",
                )?,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "json_field_number_bounds" | "json_number_bounds" => {
            Ok(BenchmarkTaskPromptScoringTemplateYaml {
                expect_json: true,
                json_field_number_bounds: expected_json_number_bounds(
                    expected,
                    "json_field_number_bounds",
                )?,
                ..BenchmarkTaskPromptScoringTemplateYaml::default()
            })
        }
        "non_empty" | "non-empty" | "nonempty" => {
            Ok(BenchmarkTaskPromptScoringTemplateYaml::default())
        }
        other => Err(format!(
            "unsupported prompt task scoring method {}; use exact, contains, regex, json, json_field_equals, json_field_contains, json_field_array_exact, json_field_array_exact_ordered, json_field_object_keys_exact, json_field_number_close, json_field_number_bounds, or non_empty",
            other
        )),
    }
}

fn prompt_task_scoring_spec_from_request(
    request: &AddBenchmarkPackPromptTaskRequest,
) -> Result<ScoringSpec, String> {
    let scoring = prompt_task_scoring_from_request(request)?;
    let value = serde_json::to_value(scoring).map_err(|err| err.to_string())?;
    serde_json::from_value(value).map_err(|err| err.to_string())
}

fn expected_json_object(
    expected: &str,
    method: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    if expected.trim().is_empty() {
        return Err(format!(
            "{} scoring requires an expected JSON object",
            method
        ));
    }
    let value: serde_json::Value = serde_json::from_str(expected)
        .map_err(|err| format!("{} expected JSON object is invalid: {}", method, err))?;
    match value {
        serde_json::Value::Object(map) if !map.is_empty() => Ok(map),
        serde_json::Value::Object(_) => Err(format!(
            "{} scoring requires at least one field path",
            method
        )),
        _ => Err(format!(
            "{} scoring requires a JSON object keyed by field path",
            method
        )),
    }
}

fn expected_json_field_values(
    expected: &str,
    method: &str,
) -> Result<BTreeMap<String, serde_json::Value>, String> {
    Ok(expected_json_object(expected, method)?
        .into_iter()
        .collect())
}

fn expected_json_string_lists(
    expected: &str,
    method: &str,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    expected_json_object(expected, method)?
        .into_iter()
        .map(|(path, value)| {
            let values = json_value_to_string_list(&value).ok_or_else(|| {
                format!(
                    "{} expected value for {} must be a string or array of strings",
                    method, path
                )
            })?;
            if values.is_empty() {
                Err(format!(
                    "{} expected value for {} must include at least one string",
                    method, path
                ))
            } else {
                Ok((path, values))
            }
        })
        .collect()
}

fn expected_json_value_arrays(
    expected: &str,
    method: &str,
) -> Result<BTreeMap<String, Vec<serde_json::Value>>, String> {
    expected_json_object(expected, method)?
        .into_iter()
        .map(|(path, value)| match value {
            serde_json::Value::Array(values) if !values.is_empty() => Ok((path, values)),
            serde_json::Value::Array(_) => Err(format!(
                "{} expected array for {} must include at least one value",
                method, path
            )),
            _ => Err(format!(
                "{} expected value for {} must be an array",
                method, path
            )),
        })
        .collect()
}

fn expected_json_number_close(
    expected: &str,
    method: &str,
) -> Result<BTreeMap<String, JsonNumberCloseSpec>, String> {
    expected_json_object(expected, method)?
        .into_iter()
        .map(|(path, value)| {
            let spec = match value {
                serde_json::Value::Number(number) => JsonNumberCloseSpec {
                    expected: number.as_f64().ok_or_else(|| {
                        format!("{} expected value for {} must be finite", method, path)
                    })?,
                    tolerance: default_number_tolerance(),
                },
                serde_json::Value::Object(mut object) => {
                    let expected = object.remove("expected").ok_or_else(|| {
                        format!(
                            "{} expected object for {} must include expected",
                            method, path
                        )
                    })?;
                    let expected = expected.as_f64().ok_or_else(|| {
                        format!("{} expected value for {} must be a number", method, path)
                    })?;
                    let tolerance = object
                        .remove("tolerance")
                        .map(|value| {
                            value.as_f64().ok_or_else(|| {
                                format!("{} tolerance for {} must be a number", method, path)
                            })
                        })
                        .transpose()?
                        .unwrap_or_else(default_number_tolerance);
                    JsonNumberCloseSpec {
                        expected,
                        tolerance,
                    }
                }
                _ => {
                    return Err(format!(
                        "{} expected value for {} must be a number or object",
                        method, path
                    ));
                }
            };
            if !spec.expected.is_finite() || !spec.tolerance.is_finite() || spec.tolerance < 0.0 {
                return Err(format!(
                    "{} expected/tolerance for {} must be finite and tolerance >= 0",
                    method, path
                ));
            }
            Ok((path, spec))
        })
        .collect()
}

fn expected_json_number_bounds(
    expected: &str,
    method: &str,
) -> Result<BTreeMap<String, JsonNumberBoundsSpec>, String> {
    expected_json_object(expected, method)?
        .into_iter()
        .map(|(path, value)| {
            let serde_json::Value::Object(mut object) = value else {
                return Err(format!(
                    "{} expected value for {} must be an object with min and/or max",
                    method, path
                ));
            };
            let min = object
                .remove("min")
                .map(|value| {
                    value
                        .as_f64()
                        .ok_or_else(|| format!("{} min for {} must be a number", method, path))
                })
                .transpose()?;
            let max = object
                .remove("max")
                .map(|value| {
                    value
                        .as_f64()
                        .ok_or_else(|| format!("{} max for {} must be a number", method, path))
                })
                .transpose()?;
            if min.is_none() && max.is_none() {
                return Err(format!(
                    "{} expected value for {} must include min and/or max",
                    method, path
                ));
            }
            if min.is_some_and(|value| !value.is_finite())
                || max.is_some_and(|value| !value.is_finite())
            {
                return Err(format!("{} bounds for {} must be finite", method, path));
            }
            if min.zip(max).is_some_and(|(min, max)| min > max) {
                return Err(format!("{} min for {} must be <= max", method, path));
            }
            Ok((path, JsonNumberBoundsSpec { min, max }))
        })
        .collect()
}

fn json_value_to_string_list(value: &serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::String(value) => Some(vec![value.trim().to_string()]),
        serde_json::Value::Array(values) => values
            .iter()
            .map(|value| value.as_str().map(|item| item.trim().to_string()))
            .collect(),
        _ => None,
    }
    .map(|values: Vec<String>| {
        values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect()
    })
}

fn split_expected_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn next_prompt_task_id(pack_id: &str, name: &str, existing_task_ids: &BTreeSet<String>) -> String {
    let slug = slugify_benchmark_fragment(name);
    let stem = if slug.starts_with(pack_id) {
        slug
    } else {
        format!("{}-{}", pack_id, slug)
    };
    for index in 1..=999 {
        let candidate = format!("{}-{:03}", stem, index);
        if !existing_task_ids.contains(&candidate) {
            return candidate;
        }
    }
    format!("{}-{}", stem, uuid::Uuid::new_v4())
}

fn slugify_benchmark_fragment(value: &str) -> String {
    let mut out = String::new();
    let mut previous_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_separator = false;
        } else if !previous_separator && !out.is_empty() {
            out.push('-');
            previous_separator = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "prompt".into()
    } else {
        out
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Default)]
struct PackMetadataSummary {
    task_types: Vec<String>,
    languages: Vec<String>,
    required_tools: Vec<String>,
    scoring_methods: Vec<String>,
    supported_target_kinds: Vec<String>,
    target_fit: String,
}

#[derive(Debug)]
struct PackEvidenceProfile {
    prompt_tasks: usize,
    total_task_weight: f64,
    profile: String,
    warnings: Vec<String>,
}

#[derive(Debug)]
struct PackCalibrationSummary {
    status: String,
    sample_size: Option<u64>,
    baseline_models: Vec<String>,
    last_reviewed: Option<String>,
    review_scope: Option<String>,
    quality_gates: Vec<String>,
    notes: Option<String>,
}

fn summarize_pack_metadata(pack: &BenchmarkPackSpec, tasks: &[TaskSpec]) -> PackMetadataSummary {
    let task_types = unique_strings(tasks.iter().map(|task| task.task_type.clone()));
    let languages = unique_strings(tasks.iter().filter_map(|task| task.language.clone()));
    let required_tools = unique_strings(tasks.iter().filter_map(|task| {
        task.scoring
            .command
            .first()
            .map(|command| command_tool_label(command))
    }));
    let scoring_methods = unique_strings(tasks.iter().flat_map(scoring_method_labels));
    let supported_target_kinds = supported_target_kinds_for_tasks(tasks);
    let target_fit = target_fit_for_tasks(pack, tasks);
    PackMetadataSummary {
        task_types,
        languages,
        required_tools,
        scoring_methods,
        supported_target_kinds,
        target_fit,
    }
}

fn benchmark_pack_calibration(pack: &BenchmarkPackSpec) -> PackCalibrationSummary {
    let Some(calibration) = &pack.calibration else {
        return PackCalibrationSummary {
            status: "uncalibrated".into(),
            sample_size: None,
            baseline_models: Vec::new(),
            last_reviewed: None,
            review_scope: None,
            quality_gates: Vec::new(),
            notes: None,
        };
    };
    PackCalibrationSummary {
        status: normalized_calibration_status(&calibration.status),
        sample_size: calibration.sample_size,
        baseline_models: unique_strings(
            calibration
                .baseline_models
                .iter()
                .map(|model| model.trim().to_string())
                .filter(|model| !model.is_empty()),
        ),
        last_reviewed: trimmed_optional_string(calibration.last_reviewed.as_deref()),
        review_scope: trimmed_optional_string(calibration.review_scope.as_deref()),
        quality_gates: unique_strings(calibration.quality_gates.iter().cloned()),
        notes: trimmed_optional_string(calibration.notes.as_deref()),
    }
}

fn default_user_calibration_quality_gates() -> Vec<String> {
    vec![
        "local_cloud_baseline_pair".into(),
        "provider_confirmed_model_identity".into(),
        "complete_pack_task_coverage".into(),
        "min_3_repetitions_per_task_target".into(),
        "cost_metrics_for_cloud_targets".into(),
        "single_generation_policy".into(),
        "review_before_public_leaderboard".into(),
    ]
}

fn default_calibration_status() -> String {
    "uncalibrated".into()
}

fn normalized_calibration_status(status: &str) -> String {
    let normalized = status.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "calibrated" | "pilot" | "reviewed" | "uncalibrated" => normalized,
        "" => "uncalibrated".into(),
        _ => "custom".into(),
    }
}

fn validate_calibration_review_date(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let valid_shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    if !valid_shape {
        return Err("benchmark pack calibration last reviewed date must use YYYY-MM-DD".into());
    }
    let year = value[0..4].parse::<u32>().unwrap_or(0);
    let month = value[5..7].parse::<u32>().unwrap_or(0);
    let day = value[8..10].parse::<u32>().unwrap_or(0);
    if year < 2000 || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return Err(
            "benchmark pack calibration last reviewed date must be a valid YYYY-MM-DD date".into(),
        );
    }
    Ok(())
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn trimmed_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn benchmark_pack_evidence_profile(
    pack: &BenchmarkPackSpec,
    tasks: &[TaskSpec],
    scoring_methods: &[String],
) -> PackEvidenceProfile {
    let prompt_tasks = tasks
        .iter()
        .filter(|task| task.task_type == "prompt")
        .count();
    let total_task_weight = tasks
        .iter()
        .map(|task| normalized_task_weight(task.weight))
        .sum::<f64>();
    let mut warnings = Vec::new();
    let lower_id = pack.id.to_ascii_lowercase();
    let lower_tags = pack
        .tags
        .iter()
        .map(|tag| tag.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let all_prompt = !tasks.is_empty() && prompt_tasks == tasks.len();
    let weak_scoring = !scoring_methods.is_empty()
        && scoring_methods
            .iter()
            .all(|method| method == "non-empty response");
    let connectivity_like =
        lower_id.contains("connectivity") || lower_tags.iter().any(|tag| tag == "connectivity");
    let smoke_like = lower_tags.iter().any(|tag| tag == "smoke");

    let profile = if tasks.is_empty() {
        warnings.push("Pack has no runnable tasks.".into());
        "empty"
    } else if tasks
        .iter()
        .any(|task| task.task_type == "benchmark_harness")
    {
        if !pack.requires_sandbox {
            warnings
                .push("Worker-backed harness packs should declare sandbox requirements.".into());
        }
        "worker_harness"
    } else if all_prompt {
        if connectivity_like {
            warnings.push(
                "Connectivity smoke confirms endpoint response; use a broader prompt pack before model selection."
                    .into(),
            );
            "connectivity_smoke"
        } else if prompt_tasks < 3 {
            warnings.push(
                "Fewer than 3 prompt tasks; run a broader pack before choosing between models."
                    .into(),
            );
            "prompt_smoke"
        } else if weak_scoring {
            warnings.push(
                "All prompt tasks use non-empty scoring; add exact, JSON, regex, or numeric checks for reliable comparison."
                    .into(),
            );
            "weak_prompt_suite"
        } else if total_task_weight < 3.0 {
            warnings.push(
                "Total task weight is below 3; add or weight tasks before treating results as model-selection evidence."
                    .into(),
            );
            "thin_prompt_suite"
        } else {
            "prompt_comparison"
        }
    } else if smoke_like {
        warnings.push(
            "Smoke packs verify the runner path; use broader packs for model-selection evidence."
                .into(),
        );
        "code_smoke"
    } else {
        "code_agent"
    };

    PackEvidenceProfile {
        prompt_tasks,
        total_task_weight,
        profile: profile.into(),
        warnings,
    }
}

fn scoring_method_labels(task: &TaskSpec) -> Vec<String> {
    let mut labels = Vec::new();
    if let Some(parse) = &task.scoring.parse {
        labels.push(parse.clone());
    }
    if task.scoring.expect_exact.is_some() {
        labels.push("exact".into());
    }
    if !task.scoring.expect_contains.is_empty() {
        labels.push("contains".into());
    }
    if !task.scoring.expect_regex.is_empty() {
        labels.push("regex".into());
    }
    if task.scoring.expect_json {
        labels.push("json".into());
    }
    if !task.scoring.json_field_equals.is_empty() {
        labels.push("json fields".into());
    }
    if !task.scoring.json_field_contains.is_empty() {
        labels.push("json field contains".into());
    }
    if !task.scoring.json_field_object_keys_exact.is_empty() {
        labels.push("exact JSON object keys".into());
    }
    if !task.scoring.json_field_array_exact.is_empty() {
        labels.push("exact JSON arrays".into());
    }
    if !task.scoring.json_field_array_exact_ordered.is_empty() {
        labels.push("ordered JSON arrays".into());
    }
    if !task.scoring.json_field_number_close.is_empty() {
        labels.push("numeric tolerance".into());
    }
    if !task.scoring.json_field_number_bounds.is_empty() {
        labels.push("numeric bounds".into());
    }
    if labels.is_empty() {
        labels.push("non-empty response".into());
    }
    labels
}

fn unique_strings(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut values = values
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn command_tool_label(command: &str) -> String {
    if command == "benchforge-worker" {
        "BenchForge worker".into()
    } else {
        command.into()
    }
}

fn supported_target_kinds_for_tasks(tasks: &[TaskSpec]) -> Vec<String> {
    let Some(first_task) = tasks.first() else {
        return vec!["mock".into()];
    };
    let mut supported = supported_target_kinds_for_task(first_task);
    for task in tasks.iter().skip(1) {
        let task_supported = supported_target_kinds_for_task(task);
        supported.retain(|kind| task_supported.iter().any(|task_kind| task_kind == kind));
    }
    supported
}

fn supported_target_kinds_for_task(task: &TaskSpec) -> Vec<String> {
    match task.task_type.as_str() {
        "prompt" => vec![
            "direct_model".into(),
            "harnessed_model".into(),
            "mock".into(),
        ],
        "benchmark_harness" => vec!["benchmark_harness".into()],
        _ => vec![
            "cli_agent".into(),
            "direct_model".into(),
            "harnessed_model".into(),
            "mock".into(),
        ],
    }
}

fn target_fit_for_tasks(pack: &BenchmarkPackSpec, tasks: &[TaskSpec]) -> String {
    if tasks.iter().all(|task| task.task_type == "prompt") {
        return "Local/cloud chat models and OpenAI-compatible runtimes".into();
    }
    if tasks
        .iter()
        .any(|task| task.task_type == "benchmark_harness")
    {
        return "External worker-backed harness; check required tools before running".into();
    }
    if pack.requires_sandbox {
        "Repo/code-edit agents or model edit targets; sandbox recommended".into()
    } else {
        "Repo/code-edit agents or model edit targets".into()
    }
}

pub fn validate_docker_scoring_preflight_for_tasks(
    tasks: &[TaskSpec],
    docker: bool,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    validate_docker_scoring_preflight(tasks, docker, is_cancelled)
}

fn normalized_run_target_ids(target_ids: &[String]) -> Vec<String> {
    if target_ids.is_empty() {
        vec!["mock-agent".to_string()]
    } else {
        target_ids.to_vec()
    }
}

pub fn validate_target_runtime_preflight_for_tasks(
    available_targets: &[store::TargetRecord],
    target_ids: &[String],
    tasks: &[TaskSpec],
) -> Result<(), String> {
    validate_target_runtime_preflight_inner(available_targets, target_ids, tasks)
}

pub fn validate_target_compatibility(
    pack: &BenchmarkPackSpec,
    tasks: &[TaskSpec],
    available_targets: &[store::TargetRecord],
    target_ids: &[String],
) -> Result<(), String> {
    let supported_kinds = supported_target_kinds_for_tasks(tasks);
    let mut missing = Vec::new();
    let mut incompatible = Vec::new();
    let mut disabled = Vec::new();

    for target_id in normalized_run_target_ids(target_ids) {
        let Some(target) = available_targets
            .iter()
            .find(|target| target.id == target_id)
        else {
            missing.push(target_id);
            continue;
        };

        if !target.enabled {
            disabled.push(target.id.clone());
            continue;
        }

        if !supported_kinds.iter().any(|kind| kind == &target.kind) {
            incompatible.push(format!("{} is {}", target.id, target.kind));
        }
    }

    if !missing.is_empty() {
        return Err(format!("target_not_found: {}", missing.join(", ")));
    }
    if !disabled.is_empty() {
        return Err(format!(
            "target_disabled: {}. Re-create or edit the target before queueing a benchmark.",
            disabled.join(", ")
        ));
    }
    if !incompatible.is_empty() {
        return Err(format!(
            "incompatible_target: pack {} supports {} target(s), but {}. Choose a compatible target or benchmark pack.",
            pack.id,
            supported_kinds.join(", "),
            incompatible.join("; ")
        ));
    }

    Ok(())
}

fn validate_target_runtime_preflight_inner(
    available_targets: &[store::TargetRecord],
    target_ids: &[String],
    tasks: &[TaskSpec],
) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut errors = Vec::new();

    for target_id in normalized_run_target_ids(target_ids) {
        let Some(target) = available_targets
            .iter()
            .find(|target| target.id == target_id)
        else {
            missing.push(target_id);
            continue;
        };

        if target.validation_status.as_deref() == Some("error") {
            errors.push(target_validation_preflight_error(target));
            continue;
        }

        if target.kind == "benchmark_harness" {
            errors.extend(benchmark_harness_preflight_errors(target, tasks));
            continue;
        }
        if !matches!(target.kind.as_str(), "direct_model" | "harnessed_model") {
            continue;
        }
        let Some(adapter) = adapters::find_adapter(&target.adapter_id)? else {
            errors.push(format!(
                "{}: adapter {} not found",
                target.id, target.adapter_id
            ));
            continue;
        };
        let config: serde_json::Value =
            serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));
        if !target_requires_api_key(&adapter.spec, &config) {
            continue;
        }
        if !configured_api_key_available(&adapter.spec, &config) {
            errors.push(format!(
                "{}: missing_key for adapter {}; {}",
                target.id,
                adapter.spec.id,
                cloud_key_remediation(&adapter.spec, &config)
            ));
        }
    }

    if !missing.is_empty() {
        return Err(format!("target_not_found: {}", missing.join(", ")));
    }
    if !errors.is_empty() {
        return Err(format!("target_preflight_failed: {}", errors.join("; ")));
    }

    Ok(())
}

fn target_validation_preflight_error(target: &store::TargetRecord) -> String {
    let detail = target
        .validation_detail
        .as_deref()
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .unwrap_or("last validation failed");
    format!(
        "{}: target_validation_failed; {}. Validate or edit the target before queueing a benchmark.",
        target.id, detail
    )
}

fn benchmark_harness_preflight_errors(
    target: &store::TargetRecord,
    tasks: &[TaskSpec],
) -> Vec<String> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let config: serde_json::Value =
        serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));
    let mut errors = Vec::new();
    for task in tasks
        .iter()
        .filter(|task| task.task_type == "benchmark_harness")
    {
        let Some(worker_kind) = worker_harness_kind(task) else {
            continue;
        };
        if worker_kind == "security" || worker_kind == "mock" {
            continue;
        }
        if let Some(command) = harness_command_executable(&config, worker_kind) {
            if !command.contains('{') && !adapters::command_exists(&command) {
                push_unique_error(
                    &mut errors,
                    format!(
                        "{}: tool_missing for {}; harness command executable not found: {}",
                        target.id, worker_kind, command
                    ),
                );
            }
            continue;
        }
        if worker_kind == "evalplus" {
            if let Some(tool) = evalplus_default_tool(&config) {
                if !tool.contains('{') && !adapters::command_exists(&tool) {
                    push_unique_error(
                        &mut errors,
                        format!(
                            "{}: tool_missing for evalplus; EvalPlus tool not found: {}",
                            target.id, tool
                        ),
                    );
                }
                continue;
            }
        }
        push_unique_error(
            &mut errors,
            format!(
                "{}: configuration_missing for {}; configure target harness.command before running task {}",
                target.id, worker_kind, task.id
            ),
        );
    }
    errors
}

fn push_unique_error(errors: &mut Vec<String>, error: String) {
    if !errors.iter().any(|existing| existing == &error) {
        errors.push(error);
    }
}

fn worker_harness_kind(task: &TaskSpec) -> Option<&str> {
    task.scoring
        .command
        .windows(2)
        .find(|window| window[0] == "--kind")
        .map(|window| window[1].as_str())
}

fn harness_command_executable(config: &serde_json::Value, worker_kind: &str) -> Option<String> {
    harness_command_value(config, worker_kind)
        .and_then(command_executable_from_value)
        .filter(|command| !command.trim().is_empty())
}

fn harness_command_value<'a>(
    config: &'a serde_json::Value,
    worker_kind: &str,
) -> Option<&'a serde_json::Value> {
    harness_settings(config, worker_kind)
        .and_then(|settings| settings.get("command"))
        .or_else(|| config.get("harness_command"))
}

fn harness_settings<'a>(
    config: &'a serde_json::Value,
    worker_kind: &str,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    config
        .get("harness")
        .and_then(|settings| settings.as_object())
        .or_else(|| {
            let normalized = worker_kind.replace('-', "_");
            config
                .get(normalized)
                .and_then(|settings| settings.as_object())
        })
}

fn command_executable_from_value(value: &serde_json::Value) -> Option<String> {
    if let Some(command) = value.as_str() {
        return command.split_whitespace().next().map(str::to_string);
    }
    value
        .as_array()
        .and_then(|items| items.first())
        .and_then(|first| first.as_str())
        .map(str::to_string)
}

fn evalplus_default_tool(config: &serde_json::Value) -> Option<String> {
    let settings = harness_settings(config, "evalplus")?;
    let has_samples = settings
        .get("samples")
        .or_else(|| settings.get("samples_path"))
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty());
    if !has_samples {
        return None;
    }
    settings
        .get("tool")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| Some("evalplus.evaluate".to_string()))
}

fn adapter_requires_cloud_key(adapter: &adapters::AdapterSpec) -> bool {
    matches!(
        adapter.kind.as_str(),
        "openai_responses" | "anthropic_messages" | "mistral_api" | "azure_openai"
    ) || adapter.validation.get("secret_env").is_some()
}

fn target_requires_api_key(adapter: &adapters::AdapterSpec, config: &serde_json::Value) -> bool {
    adapter_requires_cloud_key(adapter)
        || (adapter.kind == "openai_compatible" && target_base_url_is_remote(adapter, config))
}

fn target_base_url_is_remote(adapter: &adapters::AdapterSpec, config: &serde_json::Value) -> bool {
    config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
        .is_some_and(|base_url| {
            let lower = base_url.to_ascii_lowercase();
            (lower.starts_with("http://") || lower.starts_with("https://"))
                && !target_base_url_is_local(&lower)
        })
}

fn target_base_url_is_local(lower_base_url: &str) -> bool {
    lower_base_url.contains("://localhost")
        || lower_base_url.contains("://127.0.0.1")
        || lower_base_url.contains("://0.0.0.0")
}

fn configured_api_key_available(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> bool {
    config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        .is_some_and(secrets::cloud_api_key_available)
        || configured_api_key_env(adapter, config)
            .as_deref()
            .is_some_and(env_var_is_available)
}

fn configured_api_key_env(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Option<String> {
    config
        .get("api_key_env")
        .and_then(|value| value.as_str())
        .or_else(|| {
            adapter
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())
        })
        .map(str::to_string)
}

fn env_var_is_available(name: &str) -> bool {
    std::env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn cloud_key_remediation(adapter: &adapters::AdapterSpec, config: &serde_json::Value) -> String {
    let keychain = config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        .unwrap_or(adapter.id.as_str());
    match configured_api_key_env(adapter, config) {
        Some(secret_env) => format!(
            "save a key for {} in Keychain or set {} before starting the run",
            keychain, secret_env
        ),
        None => format!("save a key for {} in Keychain", keychain),
    }
}

fn validate_docker_scoring_preflight(
    tasks: &[TaskSpec],
    docker: bool,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    ensure_not_cancelled(is_cancelled)?;
    if !docker {
        return Ok(());
    }
    let docker_tasks = tasks
        .iter()
        .filter(|task| task_supports_docker_scoring(task))
        .collect::<Vec<_>>();
    if docker_tasks.is_empty() {
        return Ok(());
    }
    if !adapters::command_exists("docker") {
        return Err(docker_preflight_error(
            &docker_tasks,
            "docker CLI is not available. Install Docker Desktop or run `brew install colima docker`.",
        ));
    }

    let capture = run_command_capture_checked(
        command_at(
            &paths::resource_root(),
            "docker",
            &["info", "--format", "{{.ServerVersion}}"],
        ),
        Duration::from_secs(10),
        is_cancelled,
    )
    .map_err(|err| docker_preflight_error(&docker_tasks, &err))?;

    if capture.timed_out {
        return Err(docker_preflight_error(
            &docker_tasks,
            "docker info timed out after 10 seconds.",
        ));
    }
    if capture.code.unwrap_or(1) != 0 {
        let detail = first_nonempty_line(&capture.stderr)
            .or_else(|| first_nonempty_line(&capture.stdout))
            .unwrap_or_else(|| "Docker daemon is not reachable.".into());
        return Err(docker_preflight_error(&docker_tasks, &detail));
    }
    Ok(())
}

fn docker_preflight_error(tasks: &[&TaskSpec], detail: &str) -> String {
    let mut task_ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>();
    task_ids.sort_unstable();
    task_ids.dedup();
    format!(
        "docker_unavailable: Docker scoring requested for Python task(s) {}, but Docker is not ready: {} Start Docker Desktop or run `colima start`, then retry. Disable Docker scoring to use sanitized host scoring.",
        task_ids.join(", "),
        detail.trim()
    )
}

fn first_nonempty_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn task_supports_docker_scoring(task: &TaskSpec) -> bool {
    task.task_type != "prompt" && task.language.as_deref() == Some("python")
}

fn uses_docker_scoring(docker: bool, task: &TaskSpec) -> bool {
    docker && task_supports_docker_scoring(task)
}

pub fn run_quick_smoke(
    conn: &Connection,
    request: RunQuickSmokeRequest,
) -> Result<Vec<RunResultDto>, String> {
    run_quick_smoke_with_progress(conn, request, |_| {})
}

pub fn run_quick_smoke_with_progress(
    conn: &Connection,
    request: RunQuickSmokeRequest,
    progress: impl FnMut(RunProgressDto),
) -> Result<Vec<RunResultDto>, String> {
    run_quick_smoke_with_shared_cancel(conn, request, progress, Arc::new(|| false))
}

pub fn run_quick_smoke_with_shared_cancel(
    conn: &Connection,
    request: RunQuickSmokeRequest,
    progress: impl FnMut(RunProgressDto),
    is_cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Vec<RunResultDto>, String> {
    if normalized_concurrency(request.concurrency) <= 1 {
        return run_quick_smoke_with_cancel(conn, request, progress, || is_cancelled());
    }
    run_quick_smoke_parallel(conn, request, progress, is_cancelled)
}

pub fn run_quick_smoke_with_cancel(
    conn: &Connection,
    request: RunQuickSmokeRequest,
    mut progress: impl FnMut(RunProgressDto),
    is_cancelled: impl Fn() -> bool,
) -> Result<Vec<RunResultDto>, String> {
    ensure_not_cancelled(&is_cancelled)?;
    let targets = store::list_targets(conn).map_err(|err| err.to_string())?;
    let target_ids = normalized_run_target_ids(&request.target_ids);
    let pack = load_pack(&request.benchmark_pack_id)?;
    let tasks = select_tasks_for_run(load_tasks(&pack)?, &request.task_ids)?;
    validate_target_compatibility(&pack, &tasks, &targets, &target_ids)?;
    validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &tasks)?;
    validate_docker_scoring_preflight(&tasks, request.docker, &is_cancelled)?;
    let repetitions = request.repetitions.max(1);
    let warmup_runs = request.warmup_runs.min(20);
    let concurrency = normalized_concurrency(request.concurrency);
    let total = target_ids.len().max(1) * repetitions as usize * tasks.len().max(1);
    let mut completed = 0;
    let mut results = Vec::new();
    progress(RunProgressDto {
        total,
        completed,
        current_target_id: None,
        current_task_id: None,
        message: format!("Loaded {} task(s) from {}", tasks.len(), pack.id),
    });

    for target_id in target_ids {
        ensure_not_cancelled(&is_cancelled)?;
        let Some(target) = targets.iter().find(|target| target.id == target_id) else {
            let run_id = create_run_id();
            let timestamp = store::now();
            store::insert_run_with_group(
                conn,
                &run_id,
                request.run_group_id.as_deref(),
                &target_id,
                &pack.id,
                "-",
                "error",
                &timestamp,
                &timestamp,
                Some("target_not_found"),
                Some("target not found"),
                &serde_json::json!({"docker": request.docker, "concurrency": concurrency}),
                &serde_json::json!({
                    "benchforge_version": env!("CARGO_PKG_VERSION"),
                    "target": {"id": target_id},
                    "benchmark_pack": {"id": pack.id, "version": pack.version},
                    "run": {"concurrency": concurrency}
                }),
            )
            .map_err(|err| err.to_string())?;
            results.push(RunResultDto {
                id: run_id,
                target_id: target_id.clone(),
                benchmark_pack_id: pack.id.clone(),
                task_id: "-".into(),
                status: "error".into(),
                score: None,
                wall_time_ms: 0,
                artifacts: vec![],
                warnings: vec![],
                error: Some("target_not_found".into()),
            });
            completed = (completed + repetitions as usize * tasks.len().max(1)).min(total);
            progress(RunProgressDto {
                total,
                completed,
                current_target_id: Some(target_id.clone()),
                current_task_id: None,
                message: format!("Target {} not found", target_id),
            });
            continue;
        };

        if warmup_runs > 0 {
            for index in 0..warmup_runs {
                ensure_not_cancelled(&is_cancelled)?;
                progress(RunProgressDto {
                    total,
                    completed,
                    current_target_id: Some(target.id.clone()),
                    current_task_id: None,
                    message: format!("Warming {} ({}/{})", target.name, index + 1, warmup_runs),
                });
                run_target_warmup(target, &is_cancelled)?;
            }
        }

        for _ in 0..repetitions {
            for task in &tasks {
                ensure_not_cancelled(&is_cancelled)?;
                progress(RunProgressDto {
                    total,
                    completed,
                    current_target_id: Some(target.id.clone()),
                    current_task_id: Some(task.id.clone()),
                    message: format!("Running {} on {}", task.id, target.name),
                });
                let result = run_task(
                    conn,
                    target,
                    &pack,
                    task,
                    request.docker,
                    warmup_runs,
                    concurrency,
                    request.run_group_id.as_deref(),
                    &is_cancelled,
                )?;
                results.push(result);
                completed = (completed + 1).min(total);
                ensure_not_cancelled(&is_cancelled)?;
                progress(RunProgressDto {
                    total,
                    completed,
                    current_target_id: Some(target.id.clone()),
                    current_task_id: Some(task.id.clone()),
                    message: format!("Completed {}/{} task runs", completed, total),
                });
            }
        }
    }

    Ok(results)
}

fn run_quick_smoke_parallel(
    conn: &Connection,
    request: RunQuickSmokeRequest,
    mut progress: impl FnMut(RunProgressDto),
    is_cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Vec<RunResultDto>, String> {
    ensure_not_cancelled(is_cancelled.as_ref())?;
    let targets = store::list_targets(conn).map_err(|err| err.to_string())?;
    let target_ids = normalized_run_target_ids(&request.target_ids);
    let pack = Arc::new(load_pack(&request.benchmark_pack_id)?);
    let tasks = select_tasks_for_run(load_tasks(&pack)?, &request.task_ids)?;
    validate_target_compatibility(pack.as_ref(), &tasks, &targets, &target_ids)?;
    validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &tasks)?;
    validate_docker_scoring_preflight(&tasks, request.docker, is_cancelled.as_ref())?;
    let repetitions = request.repetitions.max(1);
    let warmup_runs = request.warmup_runs.min(20);
    let concurrency = normalized_concurrency(request.concurrency) as usize;
    let total = target_ids.len().max(1) * repetitions as usize * tasks.len().max(1);
    let mut completed = 0;
    let mut results = Vec::new();
    let mut work_items = VecDeque::new();

    progress(RunProgressDto {
        total,
        completed,
        current_target_id: None,
        current_task_id: None,
        message: format!(
            "Loaded {} task(s) from {}; running up to {} at once",
            tasks.len(),
            pack.id,
            concurrency
        ),
    });

    for target_id in target_ids {
        ensure_not_cancelled(is_cancelled.as_ref())?;
        let Some(target) = targets
            .iter()
            .find(|target| target.id == target_id)
            .cloned()
        else {
            let run_id = create_run_id();
            let timestamp = store::now();
            store::insert_run_with_group(
                conn,
                &run_id,
                request.run_group_id.as_deref(),
                &target_id,
                &pack.id,
                "-",
                "error",
                &timestamp,
                &timestamp,
                Some("target_not_found"),
                Some("target not found"),
                &serde_json::json!({
                    "docker": request.docker,
                    "concurrency": concurrency
                }),
                &serde_json::json!({
                    "benchforge_version": env!("CARGO_PKG_VERSION"),
                    "target": {"id": target_id},
                    "benchmark_pack": {"id": pack.id, "version": pack.version},
                    "run": {"concurrency": concurrency}
                }),
            )
            .map_err(|err| err.to_string())?;
            results.push(RunResultDto {
                id: run_id,
                target_id: target_id.clone(),
                benchmark_pack_id: pack.id.clone(),
                task_id: "-".into(),
                status: "error".into(),
                score: None,
                wall_time_ms: 0,
                artifacts: vec![],
                warnings: vec![],
                error: Some("target_not_found".into()),
            });
            completed = (completed + repetitions as usize * tasks.len().max(1)).min(total);
            progress(RunProgressDto {
                total,
                completed,
                current_target_id: Some(target_id.clone()),
                current_task_id: None,
                message: format!("Target {} not found", target_id),
            });
            continue;
        };

        if warmup_runs > 0 {
            for index in 0..warmup_runs {
                ensure_not_cancelled(is_cancelled.as_ref())?;
                progress(RunProgressDto {
                    total,
                    completed,
                    current_target_id: Some(target.id.clone()),
                    current_task_id: None,
                    message: format!("Warming {} ({}/{})", target.name, index + 1, warmup_runs),
                });
                let cancel_check = || is_cancelled();
                run_target_warmup(&target, &cancel_check)?;
            }
        }

        for _ in 0..repetitions {
            for task in &tasks {
                work_items.push_back(ParallelWorkItem {
                    target: target.clone(),
                    pack: Arc::clone(&pack),
                    task: task.clone(),
                    docker: request.docker,
                    warmup_runs,
                    concurrency: concurrency as u32,
                    run_group_id: request.run_group_id.clone(),
                });
            }
        }
    }

    if work_items.is_empty() {
        return Ok(results);
    }

    let worker_count = concurrency.min(work_items.len()).max(1);
    let queue = Arc::new(Mutex::new(work_items));
    let abort = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let mut handles = Vec::new();

    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let tx = tx.clone();
        let abort = Arc::clone(&abort);
        let is_cancelled = Arc::clone(&is_cancelled);
        handles.push(std::thread::spawn(move || {
            let conn = match store::open_app() {
                Ok(conn) => conn,
                Err(err) => {
                    let _ = tx.send(ParallelRunMessage::Failed(err.to_string()));
                    return;
                }
            };

            loop {
                if abort.load(Ordering::SeqCst) || is_cancelled() {
                    break;
                }
                let item = match queue.lock().ok().and_then(|mut queue| queue.pop_front()) {
                    Some(item) => item,
                    None => break,
                };
                if tx
                    .send(ParallelRunMessage::Started {
                        target_id: item.target.id.clone(),
                        task_id: item.task.id.clone(),
                    })
                    .is_err()
                {
                    break;
                }
                let cancel_check = || is_cancelled();
                match run_task(
                    &conn,
                    &item.target,
                    item.pack.as_ref(),
                    &item.task,
                    item.docker,
                    item.warmup_runs,
                    item.concurrency,
                    item.run_group_id.as_deref(),
                    &cancel_check,
                ) {
                    Ok(result) => {
                        if tx.send(ParallelRunMessage::Completed(result)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        abort.store(true, Ordering::SeqCst);
                        let _ = tx.send(ParallelRunMessage::Failed(err));
                        break;
                    }
                }
            }
        }));
    }
    drop(tx);

    let mut first_error = None;
    loop {
        if first_error.is_none() && is_cancelled() {
            abort.store(true, Ordering::SeqCst);
            first_error = Some("cancelled".to_string());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(ParallelRunMessage::Started { target_id, task_id }) => {
                progress(RunProgressDto {
                    total,
                    completed,
                    current_target_id: Some(target_id.clone()),
                    current_task_id: Some(task_id.clone()),
                    message: format!("Running {} on {}", task_id, target_id),
                });
            }
            Ok(ParallelRunMessage::Completed(result)) => {
                completed = (completed + 1).min(total);
                progress(RunProgressDto {
                    total,
                    completed,
                    current_target_id: Some(result.target_id.clone()),
                    current_task_id: Some(result.task_id.clone()),
                    message: format!("Completed {}/{} task runs", completed, total),
                });
                results.push(result);
            }
            Ok(ParallelRunMessage::Failed(err)) => {
                abort.store(true, Ordering::SeqCst);
                if first_error.is_none() {
                    first_error = Some(err.clone());
                }
                progress(RunProgressDto {
                    total,
                    completed,
                    current_target_id: None,
                    current_task_id: None,
                    message: err,
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    for handle in handles {
        let _ = handle.join();
    }

    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(results)
    }
}

pub fn run_cli_smoke(docker: bool) -> Result<(), String> {
    run_cli_pack_smoke("quick-smoke", docker)
}

pub fn run_cli_prompt_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-basics", false)
}

pub fn run_cli_llm_connectivity_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-connectivity", false)
}

pub fn run_cli_llm_core_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-core", false)
}

pub fn run_cli_llm_practical_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-practical", false)
}

pub fn run_cli_llm_decision_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-decision-suite", false)
}

pub fn run_cli_llm_structured_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-structured-output", false)
}

pub fn run_cli_llm_grounded_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-grounded-context", false)
}

pub fn run_cli_llm_reliability_smoke() -> Result<(), String> {
    run_cli_pack_smoke("llm-reliability", false)
}

pub fn run_cli_code_edit_smoke() -> Result<(), String> {
    run_cli_pack_smoke("code-edit-core", false)
}

pub fn run_cli_code_edit_contract_smoke() -> Result<(), String> {
    let server = CloudContractServer::start()?;
    let _api_key = ScopedEnvVar::set(CLOUD_CONTRACT_API_KEY_ENV, "benchforge-contract-key");
    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let pack = load_pack("code-edit-core")?;
    let task = load_tasks(&pack)?
        .into_iter()
        .find(|task| task.id == "code-edit-python-config-merge-001")
        .ok_or_else(|| "code_edit_contract_failed: Python config task not found".to_string())?;
    let target = store::TargetRecord {
        id: "contract-code-edit".into(),
        name: "Contract Code Edit".into(),
        kind: "direct_model".into(),
        adapter_id: "openai-compatible".into(),
        config_json: serde_json::to_string(&serde_json::json!({
            "model": "contract-code-edit",
            "base_url": format!("{}/v1", server.base_url()),
            "api_key_env": CLOUD_CONTRACT_API_KEY_ENV,
            "temperature": 0,
            "top_p": 1,
            "max_tokens": 512,
            "timeout_seconds": 10,
            "retry_count": 0,
            "input_price_usd_per_million_tokens": 10.0,
            "output_price_usd_per_million_tokens": 20.0
        }))
        .map_err(|err| err.to_string())?,
        enabled: true,
        validation_status: None,
        validation_detail: None,
        validation_checked_at: None,
    };

    let result = run_task(&conn, &target, &pack, &task, false, 0, 1, None, &|| false)?;
    let provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    let artifacts =
        store::list_artifacts(&conn, Some(&result.id)).map_err(|err| err.to_string())?;
    validate_code_edit_contract_result(&result, &provider_results, &artifacts)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "server": "loopback-code-edit-contract",
            "result": result,
            "providerResults": provider_results,
            "artifacts": artifacts,
        }))
        .map_err(|err| err.to_string())?
    );
    Ok(())
}

pub fn run_cli_security_smoke() -> Result<(), String> {
    let conn = store::open_app().map_err(|err| err.to_string())?;
    store::upsert_target(
        &conn,
        &store::NewTarget {
            id: "benchforge-worker".into(),
            name: "BenchForge Worker".into(),
            kind: "benchmark_harness".into(),
            adapter_id: "benchforge-worker".into(),
            config: serde_json::json!({"command": worker_command()}),
        },
    )
    .map_err(|err| err.to_string())?;
    let results = run_quick_smoke(
        &conn,
        RunQuickSmokeRequest {
            target_ids: vec!["benchforge-worker".into()],
            benchmark_pack_id: "security-defensive".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    if results.iter().any(|result| result.status != "passed") {
        return Err(format!(
            "security_smoke_failed: {}",
            serde_json::to_string(&results).unwrap_or_else(|_| "results unavailable".into())
        ));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&results).map_err(|err| err.to_string())?
    );
    Ok(())
}

pub fn run_cli_worker_harness_contract_smoke() -> Result<(), String> {
    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let pack = load_pack("evalplus")?;
    let task = load_tasks(&pack)?
        .into_iter()
        .find(|task| task.id == "evalplus-humaneval-plus")
        .ok_or_else(|| "worker_harness_contract_failed: EvalPlus task not found".to_string())?;
    let script_path = write_worker_harness_contract_script()?;
    let python = worker_harness_contract_python();
    let target = store::TargetRecord {
        id: "contract-worker-harness".into(),
        name: "Contract Worker Harness".into(),
        kind: "benchmark_harness".into(),
        adapter_id: "benchforge-worker".into(),
        config_json: serde_json::to_string(&serde_json::json!({
            "harness": {
                "command": [python.clone(), script_path.to_string_lossy()],
                "timeout_seconds": 5
            }
        }))
        .map_err(|err| err.to_string())?,
        enabled: true,
        validation_status: None,
        validation_detail: None,
        validation_checked_at: None,
    };

    let command_result = run_task(&conn, &target, &pack, &task, false, 0, 1, None, &|| false)?;
    let command_provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    let command_artifacts =
        store::list_artifacts(&conn, Some(&command_result.id)).map_err(|err| err.to_string())?;
    validate_worker_harness_contract_result(
        &command_result,
        &command_provider_results,
        &command_artifacts,
    )?;

    let unparsed_script_path = write_worker_harness_unparsed_contract_script()?;
    let unparsed_target = store::TargetRecord {
        id: "contract-worker-harness-unparsed".into(),
        name: "Contract Worker Harness Unparsed".into(),
        kind: "benchmark_harness".into(),
        adapter_id: "benchforge-worker".into(),
        config_json: serde_json::to_string(&serde_json::json!({
            "harness": {
                "command": [python, unparsed_script_path.to_string_lossy()],
                "timeout_seconds": 5
            }
        }))
        .map_err(|err| err.to_string())?,
        enabled: true,
        validation_status: None,
        validation_detail: None,
        validation_checked_at: None,
    };

    let unparsed_result = run_task(
        &conn,
        &unparsed_target,
        &pack,
        &task,
        false,
        0,
        1,
        None,
        &|| false,
    )?;
    let unparsed_provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    let unparsed_artifacts =
        store::list_artifacts(&conn, Some(&unparsed_result.id)).map_err(|err| err.to_string())?;
    validate_worker_harness_unparsed_contract_result(
        &unparsed_result,
        &unparsed_provider_results,
        &unparsed_artifacts,
    )?;

    let mut import_task = task.clone();
    import_task.fixture = Some("../fixtures/import-contract".into());
    let import_target = store::TargetRecord {
        id: "contract-worker-harness-import".into(),
        name: "Contract Worker Harness Import".into(),
        kind: "benchmark_harness".into(),
        adapter_id: "benchforge-worker".into(),
        config_json: serde_json::to_string(&serde_json::json!({
            "harness": {
                "import_path": "import-results.jsonl"
            }
        }))
        .map_err(|err| err.to_string())?,
        enabled: true,
        validation_status: None,
        validation_detail: None,
        validation_checked_at: None,
    };
    let import_result = run_task(
        &conn,
        &import_target,
        &pack,
        &import_task,
        false,
        0,
        1,
        None,
        &|| false,
    )?;
    let import_provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    let import_artifacts =
        store::list_artifacts(&conn, Some(&import_result.id)).map_err(|err| err.to_string())?;
    validate_worker_harness_import_contract_result(
        &import_result,
        &import_provider_results,
        &import_artifacts,
        WorkerHarnessImportExpectation {
            path: "import-results.jsonl",
            import_format: "jsonl",
            summary_source: "json",
            status: "passed",
            score: 1.0,
            total_tests: 3,
            passed_tests: 3,
            failed_tests: 0,
        },
    )?;

    let csv_import_target = store::TargetRecord {
        id: "contract-worker-harness-import-csv".into(),
        name: "Contract Worker Harness CSV Import".into(),
        kind: "benchmark_harness".into(),
        adapter_id: "benchforge-worker".into(),
        config_json: serde_json::to_string(&serde_json::json!({
            "harness": {
                "import_path": "import-results.csv"
            }
        }))
        .map_err(|err| err.to_string())?,
        enabled: true,
        validation_status: None,
        validation_detail: None,
        validation_checked_at: None,
    };
    let csv_import_result = run_task(
        &conn,
        &csv_import_target,
        &pack,
        &import_task,
        false,
        0,
        1,
        None,
        &|| false,
    )?;
    let csv_import_provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    let csv_import_artifacts =
        store::list_artifacts(&conn, Some(&csv_import_result.id)).map_err(|err| err.to_string())?;
    validate_worker_harness_import_contract_result(
        &csv_import_result,
        &csv_import_provider_results,
        &csv_import_artifacts,
        WorkerHarnessImportExpectation {
            path: "import-results.csv",
            import_format: "csv",
            summary_source: "csv",
            status: "failed",
            score: 2.0 / 3.0,
            total_tests: 3,
            passed_tests: 2,
            failed_tests: 1,
        },
    )?;

    let xml_import_target = store::TargetRecord {
        id: "contract-worker-harness-import-xml".into(),
        name: "Contract Worker Harness XML Import".into(),
        kind: "benchmark_harness".into(),
        adapter_id: "benchforge-worker".into(),
        config_json: serde_json::to_string(&serde_json::json!({
            "harness": {
                "import_path": "import-results.xml"
            }
        }))
        .map_err(|err| err.to_string())?,
        enabled: true,
        validation_status: None,
        validation_detail: None,
        validation_checked_at: None,
    };
    let xml_import_result = run_task(
        &conn,
        &xml_import_target,
        &pack,
        &import_task,
        false,
        0,
        1,
        None,
        &|| false,
    )?;
    let xml_import_provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    let xml_import_artifacts =
        store::list_artifacts(&conn, Some(&xml_import_result.id)).map_err(|err| err.to_string())?;
    validate_worker_harness_import_contract_result(
        &xml_import_result,
        &xml_import_provider_results,
        &xml_import_artifacts,
        WorkerHarnessImportExpectation {
            path: "import-results.xml",
            import_format: "xml",
            summary_source: "junit_xml",
            status: "failed",
            score: 0.5,
            total_tests: 4,
            passed_tests: 2,
            failed_tests: 2,
        },
    )?;
    let _ = fs::remove_file(script_path);
    let _ = fs::remove_file(unparsed_script_path);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "worker": "external-harness-contract",
            "commandResult": command_result,
            "commandProviderResults": command_provider_results,
            "commandArtifacts": command_artifacts,
            "unparsedResult": unparsed_result,
            "unparsedProviderResults": unparsed_provider_results,
            "unparsedArtifacts": unparsed_artifacts,
            "importResult": import_result,
            "importProviderResults": import_provider_results,
            "importArtifacts": import_artifacts,
            "csvImportResult": csv_import_result,
            "csvImportProviderResults": csv_import_provider_results,
            "csvImportArtifacts": csv_import_artifacts,
            "xmlImportResult": xml_import_result,
            "xmlImportProviderResults": xml_import_provider_results,
            "xmlImportArtifacts": xml_import_artifacts,
        }))
        .map_err(|err| err.to_string())?
    );
    Ok(())
}

fn worker_harness_contract_python() -> String {
    let venv_python = paths::repo_root()
        .join("workers")
        .join(".venv")
        .join("bin")
        .join("python");
    if venv_python.exists() {
        venv_python.to_string_lossy().to_string()
    } else if adapters::command_exists("python3") {
        "python3".into()
    } else {
        "python".into()
    }
}

fn write_worker_harness_contract_script() -> Result<PathBuf, String> {
    fs::create_dir_all(paths::app_data_dir()).map_err(|err| err.to_string())?;
    let script_path = paths::app_data_dir().join(format!(
        "worker-harness-contract-{}.py",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(
        &script_path,
        "import json, sys\nprint(json.dumps({'total': 4, 'passed': 2, 'failed': 2, 'score': 0.5}))\nsys.exit(1)\n",
    )
    .map_err(|err| err.to_string())?;
    Ok(script_path)
}

fn write_worker_harness_unparsed_contract_script() -> Result<PathBuf, String> {
    fs::create_dir_all(paths::app_data_dir()).map_err(|err| err.to_string())?;
    let script_path = paths::app_data_dir().join(format!(
        "worker-harness-unparsed-contract-{}.py",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(
        &script_path,
        "print('completed without benchmark summary')\n",
    )
    .map_err(|err| err.to_string())?;
    Ok(script_path)
}

pub fn run_cli_cloud_contract_smoke() -> Result<(), String> {
    let server = CloudContractServer::start()?;
    let _api_key = ScopedEnvVar::set(CLOUD_CONTRACT_API_KEY_ENV, "benchforge-contract-key");
    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let targets = cloud_contract_targets(&server.base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }

    let target_ids = targets
        .iter()
        .map(|target| target.id.clone())
        .collect::<Vec<_>>();
    let results = run_quick_smoke(
        &conn,
        RunQuickSmokeRequest {
            target_ids,
            benchmark_pack_id: "cloud-contract".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    let provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    validate_cloud_contract_results(&provider_results, targets.len())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "server": "loopback-provider-contract",
            "targets": targets.len(),
            "results": results,
            "providerResults": provider_results,
        }))
        .map_err(|err| err.to_string())?
    );
    Ok(())
}

pub fn run_cli_provider_error_contract_smoke() -> Result<(), String> {
    let server = CloudContractServer::start()?;
    let _api_key = ScopedEnvVar::set(CLOUD_CONTRACT_API_KEY_ENV, "benchforge-contract-key");
    let conn = store::open_memory().map_err(|err| err.to_string())?;
    let network_base_url = reserve_unbound_loopback_base_url()?;
    let targets = cloud_contract_error_targets(&server.base_url, &network_base_url);
    for target in &targets {
        store::upsert_target(&conn, target).map_err(|err| err.to_string())?;
    }
    let pack = load_pack("cloud-contract")?;
    let tasks = load_tasks(&pack)?;
    let task = tasks
        .first()
        .ok_or_else(|| "provider_error_contract_failed: cloud-contract has no task".to_string())?;

    let mut results = Vec::new();
    for target in &targets {
        let stored = store::get_target(&conn, &target.id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| {
                format!(
                    "provider_error_contract_failed: target {} was not stored",
                    target.id
                )
            })?;
        results.push(run_prompt_task(
            &conn,
            &stored,
            &pack,
            task,
            0,
            1,
            None,
            &|| false,
        )?);
    }
    let provider_results = store::list_results(&conn).map_err(|err| err.to_string())?;
    validate_provider_error_contract_results(&provider_results)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "server": "loopback-provider-contract",
            "targets": targets.iter().map(|target| target.id.clone()).collect::<Vec<_>>(),
            "results": results,
            "providerResults": provider_results,
        }))
        .map_err(|err| err.to_string())?
    );
    Ok(())
}

fn run_cli_pack_smoke(benchmark_pack_id: &str, docker: bool) -> Result<(), String> {
    let conn = store::open_app().map_err(|err| err.to_string())?;
    let results = run_quick_smoke(
        &conn,
        RunQuickSmokeRequest {
            target_ids: vec!["mock-agent".into()],
            benchmark_pack_id: benchmark_pack_id.into(),
            task_ids: vec![],
            repetitions: 1,
            docker,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: None,
        },
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&results).map_err(|err| err.to_string())?
    );
    Ok(())
}

struct ScopedEnvVar {
    name: &'static str,
    previous: Option<OsString>,
}

impl ScopedEnvVar {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

pub(crate) struct CloudContractServer {
    base_url: String,
    shutdown: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl CloudContractServer {
    pub(crate) fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        let addr = listener.local_addr().map_err(|err| err.to_string())?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(Mutex::new(HashMap::<String, u64>::new()));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_attempts = Arc::clone(&attempts);
        let handle = std::thread::spawn(move || {
            while !thread_shutdown.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let request_attempts = Arc::clone(&thread_attempts);
                        std::thread::spawn(move || {
                            let _ = handle_cloud_contract_connection(stream, request_attempts);
                        });
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            base_url: format!("http://{}", addr),
            shutdown,
            handle: Some(handle),
        })
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for CloudContractServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub(crate) fn cloud_contract_targets(base_url: &str) -> Vec<store::NewTarget> {
    vec![
        cloud_contract_target(
            "contract-openai-compatible",
            "OpenAI-compatible contract",
            "openai-compatible",
            "contract-openai-compatible",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_retry_target(
            "contract-openai-compatible-retry",
            "OpenAI-compatible retry contract",
            "openai-compatible",
            "contract-transient-rate-limit",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_target(
            "contract-openai-responses",
            "OpenAI Responses contract",
            "openai",
            "contract-openai-responses",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_target(
            "contract-anthropic",
            "Anthropic Messages contract",
            "anthropic",
            "contract-anthropic",
            base_url,
        ),
        cloud_contract_target(
            "contract-mistral",
            "Mistral API contract",
            "mistral",
            "contract-mistral",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_target(
            "contract-openrouter",
            "OpenRouter contract",
            "openrouter",
            "contract-openrouter",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_target(
            "contract-gemini",
            "Google Gemini contract",
            "gemini",
            "contract-gemini",
            &format!("{}/gemini/openai", base_url),
        ),
        cloud_contract_target(
            "contract-azure-v1",
            "Azure OpenAI v1 contract",
            "azure-openai",
            "contract-azure-v1",
            &format!("{}/azure/openai/v1", base_url),
        ),
        cloud_contract_target(
            "contract-azure-legacy",
            "Azure OpenAI legacy contract",
            "azure-openai",
            "contract-azure-legacy",
            &format!("{}/azure", base_url),
        ),
        cloud_contract_streaming_target(
            "contract-openai-compatible-stream",
            "OpenAI-compatible streaming contract",
            "openai-compatible",
            "contract-openai-compatible-stream",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_streaming_retry_target(
            "contract-openai-compatible-stream-retry",
            "OpenAI-compatible streaming retry contract",
            "openai-compatible",
            "contract-transient-rate-limit-stream",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_streaming_target(
            "contract-openai-responses-stream",
            "OpenAI Responses streaming contract",
            "openai",
            "contract-openai-responses-stream",
            &format!("{}/v1", base_url),
        ),
        cloud_contract_streaming_target(
            "contract-gemini-stream",
            "Google Gemini streaming contract",
            "gemini",
            "contract-gemini-stream",
            &format!("{}/gemini/openai", base_url),
        ),
        cloud_contract_streaming_target(
            "contract-anthropic-stream",
            "Anthropic Messages streaming contract",
            "anthropic",
            "contract-anthropic-stream",
            base_url,
        ),
        cloud_contract_streaming_target(
            "contract-azure-v1-stream",
            "Azure OpenAI v1 streaming contract",
            "azure-openai",
            "contract-azure-v1-stream",
            &format!("{}/azure/openai/v1", base_url),
        ),
    ]
}

fn cloud_contract_target(
    id: &str,
    name: &str,
    adapter_id: &str,
    model: &str,
    base_url: &str,
) -> store::NewTarget {
    cloud_contract_target_with_streaming(id, name, adapter_id, model, base_url, false)
}

fn cloud_contract_streaming_target(
    id: &str,
    name: &str,
    adapter_id: &str,
    model: &str,
    base_url: &str,
) -> store::NewTarget {
    cloud_contract_target_with_streaming(id, name, adapter_id, model, base_url, true)
}

fn cloud_contract_retry_target(
    id: &str,
    name: &str,
    adapter_id: &str,
    model: &str,
    base_url: &str,
) -> store::NewTarget {
    let mut target = cloud_contract_target(id, name, adapter_id, model, base_url);
    if let Some(config) = target.config.as_object_mut() {
        config.insert("retry_count".into(), serde_json::json!(1));
    }
    target
}

fn cloud_contract_streaming_retry_target(
    id: &str,
    name: &str,
    adapter_id: &str,
    model: &str,
    base_url: &str,
) -> store::NewTarget {
    let mut target = cloud_contract_streaming_target(id, name, adapter_id, model, base_url);
    if let Some(config) = target.config.as_object_mut() {
        config.insert("retry_count".into(), serde_json::json!(1));
    }
    target
}

fn cloud_contract_error_targets(
    contract_base_url: &str,
    network_base_url: &str,
) -> Vec<store::NewTarget> {
    provider_error_contract_cases()
        .into_iter()
        .map(|case| {
            let base_url = if case.model == "contract-network" {
                network_base_url
            } else {
                contract_base_url
            };
            cloud_contract_error_target(
                base_url,
                case.target_id,
                case.model,
                case.timeout_seconds,
                case.retry_count,
            )
        })
        .collect()
}

fn cloud_contract_error_target(
    base_url: &str,
    id: &str,
    model: &str,
    timeout_seconds: u64,
    retry_count: u64,
) -> store::NewTarget {
    store::NewTarget {
        id: id.into(),
        name: format!("Provider error contract {}", model),
        kind: "direct_model".into(),
        adapter_id: "openai-compatible".into(),
        config: serde_json::json!({
            "model": model,
            "base_url": format!("{}/v1", base_url),
            "api_key_env": CLOUD_CONTRACT_API_KEY_ENV,
            "streaming": false,
            "temperature": 0,
            "top_p": 1,
            "max_tokens": 16,
            "timeout_seconds": timeout_seconds,
            "retry_count": retry_count
        }),
    }
}

fn reserve_unbound_loopback_base_url() -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    drop(listener);
    Ok(format!("http://127.0.0.1:{}", port))
}

#[derive(Clone, Copy)]
struct ProviderErrorContractCase {
    target_id: &'static str,
    model: &'static str,
    expected_code: &'static str,
    expected_http_status: Option<f64>,
    expected_retry_after_ms: Option<f64>,
    expected_provider_attempts: Option<f64>,
    expected_retry_delay_ms: Option<f64>,
    timeout_seconds: u64,
    retry_count: u64,
}

fn provider_error_contract_cases() -> Vec<ProviderErrorContractCase> {
    vec![
        ProviderErrorContractCase {
            target_id: "contract-auth",
            model: "contract-auth",
            expected_code: "auth",
            expected_http_status: Some(401.0),
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 10,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-rate-limit",
            model: "contract-rate-limit",
            expected_code: "rate_limit",
            expected_http_status: Some(429.0),
            expected_retry_after_ms: Some(2_000.0),
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 10,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-rate-limit-retry-exhausted",
            model: "contract-rate-limit-retry-exhausted",
            expected_code: "rate_limit",
            expected_http_status: Some(429.0),
            expected_retry_after_ms: Some(0.0),
            expected_provider_attempts: Some(2.0),
            expected_retry_delay_ms: Some(0.0),
            timeout_seconds: 10,
            retry_count: 1,
        },
        ProviderErrorContractCase {
            target_id: "contract-model-not-found",
            model: "contract-model-not-found",
            expected_code: "model_not_found",
            expected_http_status: Some(404.0),
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 10,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-context-overflow",
            model: "contract-context-overflow",
            expected_code: "context_overflow",
            expected_http_status: Some(413.0),
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 10,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-content-filter",
            model: "contract-content-filter",
            expected_code: "content_filter",
            expected_http_status: Some(400.0),
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 10,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-server-error",
            model: "contract-server-error",
            expected_code: "server_error",
            expected_http_status: Some(500.0),
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 10,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-malformed-response",
            model: "contract-malformed-response",
            expected_code: "malformed_response",
            expected_http_status: Some(200.0),
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 10,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-network",
            model: "contract-network",
            expected_code: "network",
            expected_http_status: None,
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 2,
            retry_count: 0,
        },
        ProviderErrorContractCase {
            target_id: "contract-timeout",
            model: "contract-timeout",
            expected_code: "timeout",
            expected_http_status: None,
            expected_retry_after_ms: None,
            expected_provider_attempts: Some(1.0),
            expected_retry_delay_ms: None,
            timeout_seconds: 1,
            retry_count: 0,
        },
    ]
}

fn cloud_contract_target_with_streaming(
    id: &str,
    name: &str,
    adapter_id: &str,
    model: &str,
    base_url: &str,
    streaming: bool,
) -> store::NewTarget {
    store::NewTarget {
        id: id.into(),
        name: name.into(),
        kind: "direct_model".into(),
        adapter_id: adapter_id.into(),
        config: serde_json::json!({
            "model": model,
            "base_url": base_url,
            "api_key_env": CLOUD_CONTRACT_API_KEY_ENV,
            "streaming": streaming,
            "temperature": 0,
            "top_p": 1,
            "max_tokens": 16,
            "timeout_seconds": 10,
            "retry_count": 0,
            "input_price_usd_per_million_tokens": 10.0,
            "output_price_usd_per_million_tokens": 20.0,
        }),
    }
}

pub(crate) fn validate_cloud_contract_results(
    results: &[store::ResultRecord],
    expected_count: usize,
) -> Result<(), String> {
    if results.len() != expected_count {
        return Err(format!(
            "cloud_contract_failed: expected {} result(s), got {}",
            expected_count,
            results.len()
        ));
    }
    let mut failures = Vec::new();
    for result in results {
        if result.benchmark_pack_id != "cloud-contract" {
            continue;
        }
        if result.status != "passed" {
            failures.push(format!(
                "{} status {} ({})",
                result.target_id,
                result.status,
                result.error_message.as_deref().unwrap_or("no error detail")
            ));
        }
        if result.score != Some(1.0) {
            failures.push(format!("{} score was not 1.0", result.target_id));
        }
        if result.http_status != Some(200.0) {
            failures.push(format!("{} missing HTTP 200 metric", result.target_id));
        }
        if result.prompt_tokens.unwrap_or(0.0) <= 0.0
            || result.completion_tokens.unwrap_or(0.0) <= 0.0
            || result.total_tokens.unwrap_or(0.0) <= 0.0
        {
            failures.push(format!("{} missing token metrics", result.target_id));
        }
        let expected_provider_attempts = cloud_contract_expected_provider_attempts(result);
        if result.provider_attempts != Some(expected_provider_attempts) {
            failures.push(format!(
                "{} expected {} provider attempt(s), got {:?}",
                result.target_id, expected_provider_attempts, result.provider_attempts
            ));
        }
        if cloud_contract_transient_retry_result(result)
            && result.provider_retry_after_ms != Some(0.0)
        {
            failures.push(format!(
                "{} expected Retry-After retry metric 0ms, got {:?}",
                result.target_id, result.provider_retry_after_ms
            ));
        }
        if result
            .provider_model
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            failures.push(format!("{} missing provider model", result.target_id));
        }
        if result
            .finish_reason
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            failures.push(format!("{} missing finish reason", result.target_id));
        }
        if result.output_tokens_per_second.unwrap_or(0.0) <= 0.0 {
            failures.push(format!(
                "{} missing output throughput metric",
                result.target_id
            ));
        }
        if result.cost_usd.unwrap_or(0.0) <= 0.0 {
            failures.push(format!("{} missing estimated cost", result.target_id));
        }
        let streaming = result
            .reproducibility
            .pointer("/target/config/streaming")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if streaming && result.provider_time_to_first_token_ms.unwrap_or(0.0) <= 0.0 {
            failures.push(format!(
                "{} missing streaming time-to-first-token metric",
                result.target_id
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("cloud_contract_failed: {}", failures.join("; ")))
    }
}

fn cloud_contract_expected_provider_attempts(result: &store::ResultRecord) -> f64 {
    if cloud_contract_transient_retry_result(result) {
        2.0
    } else {
        1.0
    }
}

fn cloud_contract_transient_retry_result(result: &store::ResultRecord) -> bool {
    result
        .reproducibility
        .pointer("/target/config/model")
        .and_then(|value| value.as_str())
        .is_some_and(|model| {
            matches!(
                model,
                "contract-transient-rate-limit" | "contract-transient-rate-limit-stream"
            )
        })
}

fn validate_code_edit_contract_result(
    result: &RunResultDto,
    provider_results: &[store::ResultRecord],
    artifacts: &[store::ArtifactRecord],
) -> Result<(), String> {
    let mut failures = Vec::new();
    if result.status != "passed" {
        failures.push(format!(
            "result status was {} ({})",
            result.status,
            result.error.as_deref().unwrap_or("no error detail")
        ));
    }
    if result.score != Some(1.0) {
        failures.push("result score was not 1.0".into());
    }
    for expected in [
        "model-system-prompt.txt",
        "model-prompt.txt",
        "model-output.txt",
        "raw-response.json",
        "diff.patch",
        "result.json",
    ] {
        if !result.artifacts.iter().any(|artifact| artifact == expected) {
            failures.push(format!("missing returned artifact {}", expected));
        }
    }
    let Some(record) = provider_results
        .iter()
        .find(|record| record.id == result.id)
    else {
        failures.push("stored result not found".into());
        if failures.is_empty() {
            return Ok(());
        }
        return Err(format!(
            "code_edit_contract_failed: {}",
            failures.join("; ")
        ));
    };
    if record.status != "passed" {
        failures.push(format!("stored status was {}", record.status));
    }
    if record.http_status != Some(200.0) {
        failures.push(format!("stored HTTP status was {:?}", record.http_status));
    }
    if record.prompt_tokens.unwrap_or(0.0) <= 0.0
        || record.completion_tokens.unwrap_or(0.0) <= 0.0
        || record.total_tokens.unwrap_or(0.0) <= 0.0
    {
        failures.push("missing token metrics".into());
    }
    if record.provider_attempts != Some(1.0) {
        failures.push(format!(
            "provider attempts was {:?}",
            record.provider_attempts
        ));
    }
    if record.provider_model.as_deref() != Some("contract-code-edit") {
        failures.push(format!(
            "provider model was {:?}",
            record.provider_model.as_deref()
        ));
    }
    if record.finish_reason.as_deref() != Some("stop") {
        failures.push(format!(
            "finish reason was {:?}",
            record.finish_reason.as_deref()
        ));
    }
    if record.output_tokens_per_second.unwrap_or(0.0) <= 0.0 {
        failures.push("missing output throughput".into());
    }
    if record.cost_usd.unwrap_or(0.0) <= 0.0 {
        failures.push("missing estimated cost".into());
    }
    let artifact_kinds = artifacts
        .iter()
        .map(|artifact| artifact.kind.as_str())
        .collect::<Vec<_>>();
    for expected in [
        "model_system_prompt",
        "model_prompt",
        "model_output",
        "raw_response",
        "git_diff",
        "result_json",
    ] {
        if !artifact_kinds.contains(&expected) {
            failures.push(format!("missing stored artifact kind {}", expected));
        }
    }
    if let Some(prompt_artifact) = artifacts
        .iter()
        .find(|artifact| artifact.kind == "model_prompt")
    {
        let prompt = fs::read_to_string(&prompt_artifact.path).unwrap_or_default();
        if !prompt.contains("Fix merge_config") || !prompt.contains("Workspace files:") {
            failures.push(
                "model prompt artifact did not include task prompt and workspace files".into(),
            );
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "code_edit_contract_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_worker_harness_contract_result(
    result: &RunResultDto,
    provider_results: &[store::ResultRecord],
    artifacts: &[store::ArtifactRecord],
) -> Result<(), String> {
    let mut failures = Vec::new();
    if result.status != "failed" {
        failures.push(format!("result status was {}", result.status));
    }
    if result.score != Some(0.5) {
        failures.push(format!("result score was {:?}", result.score));
    }
    if result.error.as_deref() != Some("evalplus completed with benchmark failures") {
        failures.push(format!("result error was {:?}", result.error.as_deref()));
    }
    for expected in [
        "stdout.txt",
        "stderr.txt",
        "worker-result.jsonl",
        "result.json",
        "target-config.json",
        "run-config.json",
        "evalplus-raw-output.txt",
    ] {
        if !result.artifacts.iter().any(|artifact| artifact == expected) {
            failures.push(format!("missing returned artifact {}", expected));
        }
    }
    let Some(record) = provider_results
        .iter()
        .find(|record| record.id == result.id)
    else {
        failures.push("stored result not found".into());
        return Err(format!(
            "worker_harness_contract_failed: {}",
            failures.join("; ")
        ));
    };
    if record.status != "failed" {
        failures.push(format!("stored status was {}", record.status));
    }
    if record.error_code.as_deref() != Some("benchmark_failed") {
        failures.push(format!(
            "stored error code was {:?}",
            record.error_code.as_deref()
        ));
    }
    let artifact_kinds = artifacts
        .iter()
        .map(|artifact| artifact.kind.as_str())
        .collect::<Vec<_>>();
    for expected in [
        "worker_jsonl",
        "result_json",
        "target_config",
        "run_config",
        "harness_raw_output",
    ] {
        if !artifact_kinds.contains(&expected) {
            failures.push(format!("missing stored artifact kind {}", expected));
        }
    }
    if let Some(raw_artifact) = artifacts
        .iter()
        .find(|artifact| artifact.kind == "harness_raw_output")
    {
        let raw = fs::read_to_string(&raw_artifact.path).unwrap_or_default();
        if !raw.contains("\"failed\": 2") {
            failures.push("raw harness artifact did not include fake harness output".into());
        }
    }
    if let Some(result_artifact) = artifacts
        .iter()
        .find(|artifact| artifact.kind == "result_json")
    {
        let result_json = fs::read_to_string(&result_artifact.path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if result_json
            .pointer("/metrics/total_tests")
            .and_then(|value| value.as_u64())
            != Some(4)
            || result_json
                .pointer("/metrics/failed_tests")
                .and_then(|value| value.as_u64())
                != Some(2)
            || result_json
                .pointer("/metrics/harness_exit_code")
                .and_then(|value| value.as_i64())
                != Some(1)
        {
            failures.push("result JSON did not include expected harness metrics".into());
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "worker_harness_contract_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_worker_harness_unparsed_contract_result(
    result: &RunResultDto,
    provider_results: &[store::ResultRecord],
    artifacts: &[store::ArtifactRecord],
) -> Result<(), String> {
    let mut failures = Vec::new();
    if result.status != "error" {
        failures.push(format!("unparsed result status was {}", result.status));
    }
    if result.score.is_some() {
        failures.push(format!("unparsed result score was {:?}", result.score));
    }
    if !result
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("recognizable score or test summary")
    {
        failures.push(format!(
            "unparsed result error was {:?}",
            result.error.as_deref()
        ));
    }
    for expected in [
        "stdout.txt",
        "stderr.txt",
        "worker-result.jsonl",
        "result.json",
        "target-config.json",
        "run-config.json",
        "evalplus-raw-output.txt",
    ] {
        if !result.artifacts.iter().any(|artifact| artifact == expected) {
            failures.push(format!("missing unparsed returned artifact {}", expected));
        }
    }
    let Some(record) = provider_results
        .iter()
        .find(|record| record.id == result.id)
    else {
        failures.push("stored unparsed result not found".into());
        return Err(format!(
            "worker_harness_unparsed_contract_failed: {}",
            failures.join("; ")
        ));
    };
    if record.status != "error" {
        failures.push(format!("stored unparsed status was {}", record.status));
    }
    if record.error_code.as_deref() != Some("harness_unparsed") {
        failures.push(format!(
            "stored unparsed error code was {:?}",
            record.error_code.as_deref()
        ));
    }
    if record.harness_exit_code != Some(0.0) {
        failures.push(format!(
            "stored unparsed harness exit code was {:?}",
            record.harness_exit_code
        ));
    }
    if record.score.is_some() || record.score_numeric.is_some() {
        failures.push(format!(
            "stored unparsed score was score={:?} score_numeric={:?}",
            record.score, record.score_numeric
        ));
    }
    if record.pass_fail != Some(false) {
        failures.push(format!(
            "stored unparsed pass/fail was {:?}",
            record.pass_fail
        ));
    }
    let artifact_kinds = artifacts
        .iter()
        .map(|artifact| artifact.kind.as_str())
        .collect::<Vec<_>>();
    for expected in ["worker_jsonl", "result_json", "harness_raw_output"] {
        if !artifact_kinds.contains(&expected) {
            failures.push(format!(
                "missing unparsed stored artifact kind {}",
                expected
            ));
        }
    }
    if let Some(result_artifact) = artifacts
        .iter()
        .find(|artifact| artifact.kind == "result_json")
    {
        let result_json = fs::read_to_string(&result_artifact.path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if result_json
            .pointer("/error_code")
            .and_then(|value| value.as_str())
            != Some("harness_unparsed")
            || result_json
                .pointer("/metrics/harness_exit_code")
                .and_then(|value| value.as_i64())
                != Some(0)
        {
            failures.push("unparsed result JSON did not include expected error contract".into());
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "worker_harness_unparsed_contract_failed: {}",
            failures.join("; ")
        ))
    }
}

struct WorkerHarnessImportExpectation {
    path: &'static str,
    import_format: &'static str,
    summary_source: &'static str,
    status: &'static str,
    score: f64,
    total_tests: u64,
    passed_tests: u64,
    failed_tests: u64,
}

fn validate_worker_harness_import_contract_result(
    result: &RunResultDto,
    provider_results: &[store::ResultRecord],
    artifacts: &[store::ArtifactRecord],
    expected: WorkerHarnessImportExpectation,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if result.status != expected.status {
        failures.push(format!("import result status was {}", result.status));
    }
    if result
        .score
        .is_none_or(|score| (score - expected.score).abs() > 0.000_001)
    {
        failures.push(format!("import result score was {:?}", result.score));
    }
    if expected.status == "passed" && result.error.is_some() {
        failures.push(format!("import result error was {:?}", result.error));
    } else if expected.status == "failed"
        && result.error.as_deref() != Some("evalplus imported benchmark result contains failures")
    {
        failures.push(format!(
            "import result failure error was {:?}",
            result.error.as_deref()
        ));
    }
    for expected in [
        "stdout.txt",
        "stderr.txt",
        "worker-result.jsonl",
        "result.json",
        "target-config.json",
        "run-config.json",
        "evalplus-raw-output.txt",
    ] {
        if !result.artifacts.iter().any(|artifact| artifact == expected) {
            failures.push(format!("missing imported returned artifact {}", expected));
        }
    }
    let Some(record) = provider_results
        .iter()
        .find(|record| record.id == result.id)
    else {
        failures.push("stored import result not found".into());
        return Err(format!(
            "worker_harness_import_contract_failed: {}",
            failures.join("; ")
        ));
    };
    if record.status != expected.status {
        failures.push(format!("stored import status was {}", record.status));
    }
    if record
        .reproducibility
        .pointer("/worker_import/path")
        .and_then(|value| value.as_str())
        != Some(expected.path)
        || record
            .reproducibility
            .pointer("/worker_import/format")
            .and_then(|value| value.as_str())
            != Some(expected.import_format)
        || record
            .reproducibility
            .pointer("/worker_import/read_files/0")
            .and_then(|value| value.as_str())
            != Some(expected.path)
        || record
            .reproducibility
            .pointer("/worker_import/summary_source")
            .and_then(|value| value.as_str())
            != Some(expected.summary_source)
    {
        failures.push(format!(
            "stored import reproducibility was {}",
            record.reproducibility["worker_import"]
        ));
    }
    if record.import_file_count != Some(1.0)
        || record.import_total_file_count != Some(1.0)
        || record.import_omitted_file_count != Some(0.0)
        || record.import_truncated != Some(0.0)
        || record.import_truncated_bytes != Some(0.0)
    {
        failures.push(format!(
            "stored import bounds were files={:?} total={:?} omitted={:?} truncated={:?} truncated_bytes={:?}",
            record.import_file_count,
            record.import_total_file_count,
            record.import_omitted_file_count,
            record.import_truncated,
            record.import_truncated_bytes
        ));
    }
    if expected.status == "passed" && record.error_code.is_some() {
        failures.push(format!(
            "stored import error code was {:?}",
            record.error_code.as_deref()
        ));
    } else if expected.status == "failed"
        && record.error_code.as_deref() != Some("benchmark_failed")
    {
        failures.push(format!(
            "stored import failure error code was {:?}",
            record.error_code.as_deref()
        ));
    }
    let artifact_kinds = artifacts
        .iter()
        .map(|artifact| artifact.kind.as_str())
        .collect::<Vec<_>>();
    for expected in [
        "worker_jsonl",
        "result_json",
        "target_config",
        "run_config",
        "harness_raw_output",
    ] {
        if !artifact_kinds.contains(&expected) {
            failures.push(format!(
                "missing imported stored artifact kind {}",
                expected
            ));
        }
    }
    if let Some(raw_artifact) = artifacts
        .iter()
        .find(|artifact| artifact.kind == "harness_raw_output")
    {
        let raw = fs::read_to_string(&raw_artifact.path).unwrap_or_default();
        if !raw.contains(&format!("--- imported from {} ---", expected.path)) {
            failures.push("raw imported harness artifact did not include fixture output".into());
        }
    }
    if let Some(result_artifact) = artifacts
        .iter()
        .find(|artifact| artifact.kind == "result_json")
    {
        let result_json = fs::read_to_string(&result_artifact.path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if result_json
            .get("imported")
            .and_then(|value| value.as_bool())
            != Some(true)
            || result_json
                .get("import_path")
                .and_then(|value| value.as_str())
                != Some(expected.path)
            || result_json
                .get("import_format")
                .and_then(|value| value.as_str())
                != Some(expected.import_format)
            || result_json
                .pointer("/import_formats/0")
                .and_then(|value| value.as_str())
                != Some(expected.import_format)
            || result_json
                .pointer("/import_read_files/0")
                .and_then(|value| value.as_str())
                != Some(expected.path)
            || result_json
                .get("import_source")
                .and_then(|value| value.as_str())
                != Some("file")
            || result_json
                .get("import_files")
                .and_then(|value| value.as_u64())
                != Some(1)
            || result_json
                .get("import_total_files")
                .and_then(|value| value.as_u64())
                != Some(1)
            || result_json
                .get("import_omitted_files")
                .and_then(|value| value.as_u64())
                != Some(0)
            || result_json
                .get("import_truncated")
                .and_then(|value| value.as_bool())
                != Some(false)
            || result_json
                .pointer("/tests/summary_source")
                .and_then(|value| value.as_str())
                != Some(expected.summary_source)
            || result_json
                .pointer("/metrics/total_tests")
                .and_then(|value| value.as_u64())
                != Some(expected.total_tests)
            || result_json
                .pointer("/metrics/passed_tests")
                .and_then(|value| value.as_u64())
                != Some(expected.passed_tests)
            || result_json
                .pointer("/metrics/failed_tests")
                .and_then(|value| value.as_u64())
                != Some(expected.failed_tests)
            || result_json
                .pointer("/metrics/imported")
                .and_then(|value| value.as_u64())
                != Some(1)
            || result_json
                .pointer("/metrics/import_file_count")
                .and_then(|value| value.as_u64())
                != Some(1)
            || result_json
                .pointer("/metrics/import_total_file_count")
                .and_then(|value| value.as_u64())
                != Some(1)
            || result_json
                .pointer("/metrics/import_omitted_file_count")
                .and_then(|value| value.as_u64())
                != Some(0)
            || result_json
                .pointer("/metrics/import_truncated")
                .and_then(|value| value.as_u64())
                != Some(0)
            || result_json
                .pointer("/metrics/import_truncated_bytes")
                .and_then(|value| value.as_u64())
                != Some(0)
            || result_json
                .pointer("/metrics/import_format")
                .and_then(|value| value.as_str())
                != Some(expected.import_format)
            || result_json
                .pointer("/metrics/import_source")
                .and_then(|value| value.as_str())
                != Some("file")
            || result_json
                .pointer("/metrics/import_path")
                .and_then(|value| value.as_str())
                != Some(expected.path)
            || result_json
                .pointer("/metrics/summary_source")
                .and_then(|value| value.as_str())
                != Some(expected.summary_source)
        {
            failures.push("result JSON did not include expected imported harness metrics".into());
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "worker_harness_import_contract_failed: {}",
            failures.join("; ")
        ))
    }
}

fn validate_provider_error_contract_results(results: &[store::ResultRecord]) -> Result<(), String> {
    let mut failures = Vec::new();

    for case in provider_error_contract_cases() {
        let Some(result) = results
            .iter()
            .find(|result| result.target_id == case.target_id)
        else {
            failures.push(format!("missing {} result", case.target_id));
            continue;
        };
        if result.status != "error" {
            failures.push(format!("{} status was {}", case.target_id, result.status));
        }
        if result.score != Some(0.0) {
            failures.push(format!("{} score was not 0.0", case.target_id));
        }
        if result.error_code.as_deref() != Some(case.expected_code) {
            failures.push(format!(
                "{} error code was {}",
                case.target_id,
                result.error_code.as_deref().unwrap_or("-")
            ));
        }
        if result.http_status != case.expected_http_status {
            failures.push(format!(
                "{} http status was {:?}",
                case.target_id, result.http_status
            ));
        }
        if result.provider_retry_after_ms != case.expected_retry_after_ms {
            failures.push(format!(
                "{} retry-after was {:?}",
                case.target_id, result.provider_retry_after_ms
            ));
        }
        if result.provider_attempts != case.expected_provider_attempts {
            failures.push(format!(
                "{} provider attempts were {:?}",
                case.target_id, result.provider_attempts
            ));
        }
        if result.provider_retry_delay_ms != case.expected_retry_delay_ms {
            failures.push(format!(
                "{} retry delay was {:?}",
                case.target_id, result.provider_retry_delay_ms
            ));
        }
        if let Some(expected_status) = case.expected_http_status {
            let expected_fragment = format!("http_status {}", expected_status as u16);
            if !result
                .error_message
                .as_deref()
                .unwrap_or("")
                .contains(&expected_fragment)
            {
                failures.push(format!(
                    "{} error message did not include {}",
                    case.target_id, expected_fragment
                ));
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "provider_error_contract_failed: {}",
            failures.join("; ")
        ))
    }
}

struct CloudContractHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
    streaming: bool,
    extra_headers: Vec<(String, String)>,
    delay_ms: u64,
}

fn handle_cloud_contract_connection(
    stream: TcpStream,
    attempts: Arc<Mutex<HashMap<String, u64>>>,
) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|err| err.to_string())?);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|err| err.to_string())?;
    if request_line.trim().is_empty() {
        return Ok(());
    }

    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|err| err.to_string())?;
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut request_body = String::new();
    if content_length > 0 {
        let mut body = vec![0; content_length];
        reader
            .read_exact(&mut body)
            .map_err(|err| err.to_string())?;
        request_body = String::from_utf8_lossy(&body).to_string();
    }

    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let response = cloud_contract_response(path, &request_body, &attempts);
    let status_text = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Provider Error",
    };
    let extra_headers = cloud_contract_extra_headers(&response);
    let mut stream = stream;
    if response.delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(response.delay_ms));
    }
    if response.streaming {
        let headers = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nCache-Control: no-cache\r\n{}Connection: close\r\n\r\n",
            response.status, status_text, response.content_type, extra_headers
        );
        stream
            .write_all(headers.as_bytes())
            .map_err(|err| err.to_string())?;
        for chunk in response.body.split_inclusive("\n\n") {
            stream
                .write_all(chunk.as_bytes())
                .map_err(|err| err.to_string())?;
            stream.flush().map_err(|err| err.to_string())?;
            std::thread::sleep(Duration::from_millis(10));
        }
        return stream.flush().map_err(|err| err.to_string());
    }
    let http = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{}",
        response.status,
        status_text,
        response.content_type,
        response.body.len(),
        extra_headers,
        response.body
    );
    stream
        .write_all(http.as_bytes())
        .map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())
}

fn cloud_contract_response(
    path: &str,
    request_body: &str,
    attempts: &Arc<Mutex<HashMap<String, u64>>>,
) -> CloudContractHttpResponse {
    if let Some(response) = cloud_contract_error_response(request_body, attempts) {
        return response;
    }
    let streaming = cloud_contract_request_streaming(request_body);
    if path.ends_with("/models") {
        return cloud_contract_json_response(
            200,
            serde_json::json!({
                "data": [
                    {"id": "contract-openai-compatible"},
                    {"id": "contract-no-model-echo"},
                    {"id": "contract-openai-compatible-stream"},
                    {"id": "contract-transient-rate-limit"}
                ]
            }),
        );
    }
    if cloud_contract_request_model(request_body).as_deref() == Some("contract-code-edit")
        && path.contains("/chat/completions")
    {
        if streaming {
            return cloud_contract_stream_response(openai_chat_code_edit_contract_sse());
        }
        return cloud_contract_code_edit_response();
    }
    if cloud_contract_request_model(request_body).as_deref() == Some("contract-no-model-echo")
        && path.contains("/chat/completions")
    {
        return cloud_contract_json_response(
            200,
            serde_json::json!({
                "id": "chatcmpl_contract_no_model_echo",
                "object": "chat.completion",
                "created": 1783370000,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": CLOUD_CONTRACT_EXPECTED_REPLY
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 3,
                    "total_tokens": 15
                }
            }),
        );
    }
    if streaming {
        if path.ends_with("/responses") {
            return cloud_contract_stream_response(openai_responses_contract_sse());
        }
        if path.ends_with("/v1/messages") {
            return cloud_contract_stream_response(anthropic_contract_sse());
        }
        if path.contains("/chat/completions") {
            return cloud_contract_stream_response(openai_chat_contract_sse());
        }
    }
    if path.ends_with("/responses") {
        return cloud_contract_json_response(
            200,
            serde_json::json!({
                "id": "resp_contract",
                "object": "response",
                "model": "contract-openai-responses",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "content": [{"type": "output_text", "text": CLOUD_CONTRACT_EXPECTED_REPLY}]
                }],
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 3,
                    "total_tokens": 15
                }
            }),
        );
    }
    if path.ends_with("/v1/messages") {
        return cloud_contract_json_response(
            200,
            serde_json::json!({
                "id": "msg_contract",
                "type": "message",
                "role": "assistant",
                "model": "contract-anthropic",
                "content": [{"type": "text", "text": CLOUD_CONTRACT_EXPECTED_REPLY}],
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 3
                }
            }),
        );
    }
    if path.contains("/chat/completions") {
        return cloud_contract_json_response(
            200,
            serde_json::json!({
                "id": "chatcmpl_contract",
                "object": "chat.completion",
                "created": 1783370000,
                "model": "contract-chat",
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": CLOUD_CONTRACT_EXPECTED_REPLY
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 3,
                    "total_tokens": 15
                }
            }),
        );
    }
    cloud_contract_json_response(
        404,
        serde_json::json!({
            "error": {
                "type": "not_found",
                "message": format!("unhandled contract path {}", path)
            }
        }),
    )
}

fn cloud_contract_code_edit_payload() -> String {
    serde_json::json!({
        "edits": [{
            "path": "config_merge.py",
            "content": PYTHON_CONFIG_MERGE_FIX
        }]
    })
    .to_string()
}

fn cloud_contract_code_edit_response() -> CloudContractHttpResponse {
    let edit_payload = cloud_contract_code_edit_payload();
    cloud_contract_json_response(
        200,
        serde_json::json!({
            "id": "chatcmpl_contract_code_edit",
            "object": "chat.completion",
            "created": 1783370000,
            "model": "contract-code-edit",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": edit_payload
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 64,
                "completion_tokens": 32,
                "total_tokens": 96
            }
        }),
    )
}

fn cloud_contract_error_response(
    request_body: &str,
    attempts: &Arc<Mutex<HashMap<String, u64>>>,
) -> Option<CloudContractHttpResponse> {
    match cloud_contract_request_model(request_body).as_deref()? {
        "contract-transient-rate-limit" | "contract-transient-rate-limit-stream"
            if cloud_contract_model_attempt(request_body, attempts) == 1 =>
        {
            Some(cloud_contract_json_response_with_headers(
                429,
                serde_json::json!({
                    "error": {
                        "type": "rate_limit_error",
                        "message": "contract transient rate limit"
                    }
                }),
                vec![("Retry-After".into(), "0".into())],
            ))
        }
        "contract-auth" => Some(cloud_contract_json_response(
            401,
            serde_json::json!({
                "error": {
                    "type": "authentication_error",
                    "message": "contract invalid api key"
                }
            }),
        )),
        "contract-rate-limit" => Some(cloud_contract_json_response_with_headers(
            429,
            serde_json::json!({
                "error": {
                    "type": "rate_limit_error",
                    "message": "contract rate limit"
                }
            }),
            vec![("Retry-After".into(), "2".into())],
        )),
        "contract-rate-limit-retry-exhausted" => Some(cloud_contract_json_response_with_headers(
            429,
            serde_json::json!({
                "error": {
                    "type": "rate_limit_error",
                    "message": "contract retry budget exhausted"
                }
            }),
            vec![("Retry-After".into(), "0".into())],
        )),
        "contract-model-not-found" => Some(cloud_contract_json_response(
            404,
            serde_json::json!({
                "error": {
                    "type": "model_not_found",
                    "message": "contract model does not exist"
                }
            }),
        )),
        "contract-context-overflow" => Some(cloud_contract_json_response(
            413,
            serde_json::json!({
                "error": {
                    "type": "context_length_exceeded",
                    "message": "maximum context length exceeded"
                }
            }),
        )),
        "contract-content-filter" => Some(cloud_contract_json_response(
            400,
            serde_json::json!({
                "error": {
                    "type": "content_filter",
                    "message": "blocked by safety policy"
                }
            }),
        )),
        "contract-server-error" => Some(cloud_contract_json_response(
            500,
            serde_json::json!({
                "error": {
                    "type": "server_error",
                    "message": "internal error from contract provider"
                }
            }),
        )),
        "contract-malformed-response" => Some(CloudContractHttpResponse {
            status: 200,
            content_type: "application/json",
            body: "{invalid provider json".into(),
            streaming: false,
            extra_headers: Vec::new(),
            delay_ms: 0,
        }),
        "contract-timeout" => Some(CloudContractHttpResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::json!({
                "id": "chatcmpl_contract_timeout",
                "object": "chat.completion",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": CLOUD_CONTRACT_EXPECTED_REPLY
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 12,
                    "completion_tokens": 3,
                    "total_tokens": 15
                }
            })
            .to_string(),
            streaming: false,
            extra_headers: Vec::new(),
            delay_ms: 1_500,
        }),
        _ => None,
    }
}

fn cloud_contract_model_attempt(
    request_body: &str,
    attempts: &Arc<Mutex<HashMap<String, u64>>>,
) -> u64 {
    let Some(model) = cloud_contract_request_model(request_body) else {
        return 0;
    };
    let Ok(mut attempts) = attempts.lock() else {
        return 0;
    };
    let attempt = attempts.entry(model).or_insert(0);
    *attempt += 1;
    *attempt
}

fn cloud_contract_request_streaming(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("stream").and_then(|stream| stream.as_bool()))
        .unwrap_or(false)
}

fn cloud_contract_request_model(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("model")
                .and_then(|model| model.as_str())
                .map(str::to_string)
        })
}

fn cloud_contract_json_response(status: u16, body: serde_json::Value) -> CloudContractHttpResponse {
    cloud_contract_json_response_with_headers(status, body, vec![])
}

fn cloud_contract_json_response_with_headers(
    status: u16,
    body: serde_json::Value,
    extra_headers: Vec<(String, String)>,
) -> CloudContractHttpResponse {
    CloudContractHttpResponse {
        status,
        content_type: "application/json",
        body: body.to_string(),
        streaming: false,
        extra_headers,
        delay_ms: 0,
    }
}

fn cloud_contract_stream_response(body: String) -> CloudContractHttpResponse {
    CloudContractHttpResponse {
        status: 200,
        content_type: "text/event-stream",
        body,
        streaming: true,
        extra_headers: vec![],
        delay_ms: 0,
    }
}

fn cloud_contract_extra_headers(response: &CloudContractHttpResponse) -> String {
    response
        .extra_headers
        .iter()
        .map(|(name, value)| format!("{}: {}\r\n", name, value))
        .collect()
}

fn sse_event(event: Option<&str>, data: serde_json::Value) -> String {
    let mut out = String::new();
    if let Some(event) = event {
        out.push_str("event: ");
        out.push_str(event);
        out.push('\n');
    }
    out.push_str("data: ");
    out.push_str(&data.to_string());
    out.push_str("\n\n");
    out
}

fn openai_chat_contract_sse() -> String {
    let mut out = String::new();
    out.push_str(&sse_event(
        None,
        serde_json::json!({
            "id": "chatcmpl_contract_stream",
            "object": "chat.completion.chunk",
            "created": 1783370000,
            "model": "contract-chat-stream",
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant"},
                "finish_reason": null
            }]
        }),
    ));
    out.push_str(&sse_event(
        None,
        serde_json::json!({
            "id": "chatcmpl_contract_stream",
            "object": "chat.completion.chunk",
            "created": 1783370000,
            "model": "contract-chat-stream",
            "choices": [{
                "index": 0,
                "delta": {"content": "benchforge-"},
                "finish_reason": null
            }]
        }),
    ));
    out.push_str(&sse_event(
        None,
        serde_json::json!({
            "id": "chatcmpl_contract_stream",
            "object": "chat.completion.chunk",
            "created": 1783370000,
            "model": "contract-chat-stream",
            "choices": [{
                "index": 0,
                "delta": {"content": "cloud-ok"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 3,
                "total_tokens": 15
            }
        }),
    ));
    out.push_str("data: [DONE]\n\n");
    out
}

fn openai_chat_code_edit_contract_sse() -> String {
    let mut out = String::new();
    out.push_str(&sse_event(
        None,
        serde_json::json!({
            "id": "chatcmpl_contract_code_edit_stream",
            "object": "chat.completion.chunk",
            "created": 1783370000,
            "model": "contract-code-edit",
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant"},
                "finish_reason": null
            }]
        }),
    ));
    out.push_str(&sse_event(
        None,
        serde_json::json!({
            "id": "chatcmpl_contract_code_edit_stream",
            "object": "chat.completion.chunk",
            "created": 1783370000,
            "model": "contract-code-edit",
            "choices": [{
                "index": 0,
                "delta": {"content": cloud_contract_code_edit_payload()},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 64,
                "completion_tokens": 32,
                "total_tokens": 96
            }
        }),
    ));
    out.push_str("data: [DONE]\n\n");
    out
}

fn openai_responses_contract_sse() -> String {
    let mut out = String::new();
    out.push_str(&sse_event(
        Some("response.output_text.delta"),
        serde_json::json!({
            "type": "response.output_text.delta",
            "delta": "benchforge-"
        }),
    ));
    out.push_str(&sse_event(
        Some("response.output_text.delta"),
        serde_json::json!({
            "type": "response.output_text.delta",
            "delta": "cloud-ok"
        }),
    ));
    out.push_str(&sse_event(
        Some("response.completed"),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": "resp_contract_stream",
                "object": "response",
                "model": "contract-openai-responses-stream",
                "status": "completed",
                "output": [],
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 3,
                    "total_tokens": 15
                }
            }
        }),
    ));
    out
}

fn anthropic_contract_sse() -> String {
    let mut out = String::new();
    out.push_str(&sse_event(
        Some("message_start"),
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "msg_contract_stream",
                "type": "message",
                "role": "assistant",
                "model": "contract-anthropic-stream",
                "content": [],
                "usage": {"input_tokens": 12, "output_tokens": 0}
            }
        }),
    ));
    for text in ["benchforge-", "cloud-ok"] {
        out.push_str(&sse_event(
            Some("content_block_delta"),
            serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text}
            }),
        ));
    }
    out.push_str(&sse_event(
        Some("message_delta"),
        serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 3}
        }),
    ));
    out.push_str(&sse_event(
        Some("message_stop"),
        serde_json::json!({"type": "message_stop"}),
    ));
    out
}

pub fn run_worker_mock(conn: &Connection) -> Result<RunResultDto, String> {
    let run_id = create_run_id();
    let started_at = store::now();
    let run_dir = paths::runs_dir().join(&run_id);
    let artifacts = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts).map_err(|err| err.to_string())?;
    let output_path = artifacts.join("worker-result.jsonl");
    let worker = worker_command();
    let capture = run_command_capture(
        command_at(
            &paths::resource_root(),
            &worker,
            &[
                "run",
                "--kind",
                "mock",
                "--run-id",
                &run_id,
                "--output",
                output_path.to_string_lossy().as_ref(),
            ],
        ),
        Duration::from_secs(60),
    )?;
    let stdout_path = write_artifact(&artifacts, "stdout.txt", &capture.stdout)?;
    let stderr_path = write_artifact(&artifacts, "stderr.txt", &capture.stderr)?;
    let result_jsonl = fs::read_to_string(&output_path).unwrap_or_else(|_| capture.stdout.clone());
    let mut final_event = result_jsonl
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .unwrap_or_else(|| serde_json::json!({"status": "error", "score": null, "metrics": {}}));
    let status = final_event
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("error")
        .to_string();
    let score = final_event.get("score").and_then(|value| value.as_f64());
    ensure_result_event_command_capture_metrics(&mut final_event, &capture);
    normalize_result_event_metrics(&mut final_event, &status, score);
    let wall_time_ms = final_event
        .pointer("/metrics/wall_time_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(capture.wall_time_ms);
    let finished_at = store::now();
    let reproducibility = serde_json::json!({
        "benchforge_version": env!("CARGO_PKG_VERSION"),
        "worker": worker,
        "kind": "mock",
        "sandbox": "host-worker",
        "sandbox_level": 1,
        "permission_mode": "worker-host-mock",
        "network": "host"
    });
    store::insert_run(
        conn,
        &run_id,
        "benchforge-worker",
        "worker-mock",
        "worker-mock",
        &status,
        &started_at,
        &finished_at,
        None,
        None,
        &serde_json::json!({"worker": "mock"}),
        &reproducibility,
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(conn, &run_id, "score", score, None, "worker")
        .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "wall_time_ms",
        Some(wall_time_ms as f64),
        Some("ms"),
        "worker",
    )
    .map_err(|err| err.to_string())?;
    for (kind, path) in [
        ("stdout", stdout_path),
        ("stderr", stderr_path),
        ("worker_jsonl", output_path),
    ] {
        let digest = checksum_file(&path).ok();
        store::insert_artifact(
            conn,
            &run_id,
            kind,
            &path,
            Some("text/plain"),
            digest.as_deref(),
            &serde_json::json!({}),
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(RunResultDto {
        id: run_id,
        target_id: "benchforge-worker".into(),
        benchmark_pack_id: "worker-mock".into(),
        task_id: "worker-mock".into(),
        status,
        score,
        wall_time_ms,
        artifacts: vec![
            "stdout.txt".into(),
            "stderr.txt".into(),
            "worker-result.jsonl".into(),
        ],
        warnings: vec![],
        error: None,
    })
}

fn run_benchmark_harness_task(
    conn: &Connection,
    target: &store::TargetRecord,
    pack: &BenchmarkPackSpec,
    task: &TaskSpec,
    run_group_id: Option<&str>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<RunResultDto, String> {
    ensure_not_cancelled(is_cancelled)?;
    let started_at = store::now();
    let run_id = create_run_id();
    let run_dir = paths::runs_dir().join(&run_id);
    let workspace = run_dir.join("workspace");
    let artifacts = run_dir.join("artifacts");
    fs::create_dir_all(&workspace).map_err(|err| err.to_string())?;
    fs::create_dir_all(&artifacts).map_err(|err| err.to_string())?;
    if task.fixture.is_some() {
        let fixture = resolve_fixture(task)?;
        copy_dir(&fixture, &workspace)?;
    }

    let target_config: serde_json::Value =
        serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));
    let worker_target_config_path = run_dir.join("target-config.private.json");
    let target_config_path = artifacts.join("target-config.json");
    let run_config = serde_json::json!({
        "run_id": run_id,
        "benchmark_pack_id": pack.id,
        "task_id": task.id,
        "workspace": workspace,
    });
    let run_config_path = artifacts.join("run-config.json");
    fs::write(
        &worker_target_config_path,
        serde_json::to_string_pretty(&target_config).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    fs::write(
        &target_config_path,
        serde_json::to_string_pretty(&redact_target_config(&target_config))
            .map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    fs::write(
        &run_config_path,
        serde_json::to_string_pretty(&run_config).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    let output_path = artifacts.join("worker-result.jsonl");
    let Some((command, args)) = task.scoring.command.split_first() else {
        return Err(format!("task {} has no worker command", task.id));
    };
    let command = if command == "benchforge-worker" {
        worker_command()
    } else {
        command.clone()
    };
    let scoring_command_metadata = capture_command_version_metadata_at(
        &task.scoring.command,
        &command,
        &paths::resource_root(),
        is_cancelled,
    )?;
    let mut worker_args = args.to_vec();
    worker_args.extend([
        "--run-id".to_string(),
        run_id.clone(),
        "--workspace".to_string(),
        workspace.to_string_lossy().to_string(),
        "--target-config".to_string(),
        worker_target_config_path.to_string_lossy().to_string(),
        "--run-config".to_string(),
        run_config_path.to_string_lossy().to_string(),
        "--benchmark-pack".to_string(),
        pack.id.clone(),
        "--output".to_string(),
        output_path.to_string_lossy().to_string(),
    ]);
    let arg_refs = worker_args.iter().map(String::as_str).collect::<Vec<_>>();
    let capture_result = run_command_capture_checked(
        command_at(&paths::resource_root(), &command, &arg_refs),
        Duration::from_secs(task.timeout_seconds),
        is_cancelled,
    );
    let _ = fs::remove_file(&worker_target_config_path);
    let capture = capture_result?;
    let stdout_path = write_artifact(&artifacts, "stdout.txt", &capture.stdout)?;
    let stderr_path = write_artifact(&artifacts, "stderr.txt", &capture.stderr)?;
    let result_jsonl = fs::read_to_string(&output_path).unwrap_or_else(|_| capture.stdout.clone());
    let mut final_event = parse_worker_final_event(&result_jsonl).unwrap_or_else(
        || serde_json::json!({"status": "error", "score": null, "metrics": {}, "safety": {}}),
    );
    if capture.timed_out {
        final_event["status"] = serde_json::json!("timeout");
        final_event["score"] = serde_json::json!(0.0);
        final_event["metrics"]["wall_time_ms"] = serde_json::json!(capture.wall_time_ms);
    } else if capture.code.unwrap_or(1) != 0
        && final_event
            .get("status")
            .and_then(|value| value.as_str())
            .is_none()
    {
        final_event["status"] = serde_json::json!("error");
        final_event["score"] = serde_json::Value::Null;
    }

    let status = final_event
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("error")
        .to_string();
    let score = final_event.get("score").and_then(|value| value.as_f64());
    ensure_result_event_command_capture_metrics(&mut final_event, &capture);
    normalize_result_event_metrics(&mut final_event, &status, score);
    let wall_time_ms = final_event
        .pointer("/metrics/wall_time_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(capture.wall_time_ms);
    let finding_count = final_event
        .pointer("/metrics/finding_count")
        .and_then(|value| value.as_f64());
    let files_scanned = final_event
        .pointer("/metrics/files_scanned")
        .and_then(|value| value.as_f64());
    let mut warnings = worker_diagnostics(&final_event);
    if finding_count.unwrap_or(0.0) > 0.0 {
        warnings.push(format!(
            "security_findings_detected: {}",
            finding_count.unwrap_or(0.0) as u64
        ));
    }
    if capture.code.unwrap_or(0) != 0 && status == "error" {
        warnings.push(format!(
            "worker_exit_code: {}",
            capture.code.unwrap_or_default()
        ));
    }
    let error_code = worker_run_error_code(&status, &final_event, finding_count);
    let error_message =
        worker_run_error_message(&status, &final_event, finding_count, error_code.as_deref());

    let result_path = write_artifact(
        &artifacts,
        "result.json",
        &serde_json::to_string_pretty(&final_event).map_err(|err| err.to_string())?,
    )?;
    if !output_path.exists() {
        fs::write(&output_path, &result_jsonl).map_err(|err| err.to_string())?;
    }
    let finished_at = store::now();
    let benchmark_pack = benchmark_pack_reproducibility(pack)?;
    let prompts = prompt_reproducibility(optional_prompt(&task.prompt), None, None, None);
    let mut reproducibility = serde_json::json!({
        "benchforge_version": env!("CARGO_PKG_VERSION"),
        "benchmark_pack": benchmark_pack,
        "task": {"id": task.id, "version": task.version, "weight": normalized_task_weight(task.weight), "checksum": checksum_file(&task.source_path)?},
        "target": target_reproducibility(target, &target_config),
        "prompts": prompts,
        "worker": command,
        "scoring_command_metadata": &scoring_command_metadata,
        "run": {"worker_args": worker_args},
        "sandbox": "worker-workspace",
        "sandbox_level": 1,
        "permission_mode": "worker-harness-minimal-env",
        "network": "host",
        "environment": "benchforge-worker",
        "workspace_path": workspace.to_string_lossy(),
        "host": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    });
    if let Some(worker_import) = worker_import_reproducibility(&final_event) {
        reproducibility["worker_import"] = worker_import;
    }

    store::insert_run_with_group(
        conn,
        &run_id,
        run_group_id,
        &target.id,
        &pack.id,
        &task.id,
        &status,
        &started_at,
        &finished_at,
        error_code.as_deref(),
        error_message.as_deref(),
        &serde_json::json!({"worker": true}),
        &reproducibility,
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(conn, &run_id, "score", score, None, "worker")
        .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "wall_time_ms",
        Some(wall_time_ms as f64),
        Some("ms"),
        "worker",
    )
    .map_err(|err| err.to_string())?;
    if let Some(metrics) = final_event
        .get("metrics")
        .and_then(|value| value.as_object())
    {
        for (name, value) in metrics {
            if name == "wall_time_ms" {
                continue;
            }
            if let Some(number) = value.as_f64() {
                store::insert_metric(conn, &run_id, name, Some(number), None, "worker")
                    .map_err(|err| err.to_string())?;
            } else if let Some(text) = value.as_str() {
                store::insert_metric_text(conn, &run_id, name, text, "worker")
                    .map_err(|err| err.to_string())?;
            }
        }
    }
    if let Some(value) = finding_count {
        store::insert_metric(
            conn,
            &run_id,
            "security_finding_count",
            Some(value),
            None,
            "worker",
        )
        .map_err(|err| err.to_string())?;
    }
    if let Some(value) = files_scanned {
        store::insert_metric(
            conn,
            &run_id,
            "security_files_scanned",
            Some(value),
            None,
            "worker",
        )
        .map_err(|err| err.to_string())?;
    }
    let mut artifact_records = vec![
        ("stdout".to_string(), stdout_path, "text/plain"),
        ("stderr".to_string(), stderr_path, "text/plain"),
        (
            "worker_jsonl".to_string(),
            output_path,
            "application/x-jsonlines",
        ),
        ("result_json".to_string(), result_path, "application/json"),
        (
            "scoring_command".to_string(),
            write_artifact(
                &artifacts,
                "scoring-command.json",
                &serde_json::to_string_pretty(&scoring_command_metadata)
                    .map_err(|err| err.to_string())?,
            )?,
            "application/json",
        ),
        (
            "target_config".to_string(),
            target_config_path,
            "application/json",
        ),
        (
            "run_config".to_string(),
            run_config_path,
            "application/json",
        ),
    ];
    artifact_records.extend(
        worker_declared_artifacts(&final_event, &run_dir)
            .into_iter()
            .map(|(kind, path)| (kind, path, "text/plain")),
    );
    let artifact_names = artifact_records
        .iter()
        .filter_map(|(_, path, _)| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .collect::<Vec<_>>();
    for (kind, path, mime) in artifact_records {
        let digest = checksum_file(&path).ok();
        store::insert_artifact(
            conn,
            &run_id,
            &kind,
            &path,
            Some(mime),
            digest.as_deref(),
            &serde_json::json!({}),
        )
        .map_err(|err| err.to_string())?;
    }

    Ok(RunResultDto {
        id: run_id,
        target_id: target.id.clone(),
        benchmark_pack_id: pack.id.clone(),
        task_id: task.id.clone(),
        status,
        score,
        wall_time_ms,
        artifacts: artifact_names,
        warnings,
        error: error_message,
    })
}

fn run_prompt_task(
    conn: &Connection,
    target: &store::TargetRecord,
    pack: &BenchmarkPackSpec,
    task: &TaskSpec,
    warmup_runs: u32,
    concurrency: u32,
    run_group_id: Option<&str>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<RunResultDto, String> {
    ensure_not_cancelled(is_cancelled)?;
    let setup_started = Instant::now();
    let started_at = store::now();
    let run_id = create_run_id();
    let run_dir = paths::runs_dir().join(&run_id);
    let artifacts = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts).map_err(|err| err.to_string())?;
    let target_config: serde_json::Value =
        serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));
    let setup_time_ms = setup_started.elapsed().as_millis() as u64;

    let call_started = Instant::now();
    let output = run_prompt_target(target, task, is_cancelled);
    let wall_time_ms = call_started.elapsed().as_millis() as u64;
    let target_time_ms = wall_time_ms;
    let secret_values = collect_secret_values();

    let (content, raw_response, mut provider_metrics, prompt_score, error_code) = match output {
        Ok(output) => {
            let content = safety::redact_secrets(&output.content, &secret_values);
            let raw_response = output
                .raw_response
                .map(|raw| safety::redact_secrets(&raw, &secret_values));
            let prompt_score = score_prompt_response(&task.scoring, &content);
            let error_code = prompt_failure_error_code(&prompt_score.tests).map(str::to_string);
            (
                content,
                raw_response,
                output.metrics,
                prompt_score,
                error_code,
            )
        }
        Err(err) => {
            let error_code = normalize_provider_error_code(&err).to_string();
            let provider_metrics = provider_error_transport_metrics(&err);
            (
                String::new(),
                None,
                provider_metrics,
                PromptScore {
                    status: "error".into(),
                    score: 0.0,
                    tests: serde_json::json!({"error": err, "error_code": error_code}),
                    error_message: Some(err),
                },
                Some(error_code),
            )
        }
    };
    ensure_provider_model_metric(&mut provider_metrics, &target_config);

    let prompt_path = write_artifact(&artifacts, "prompt.txt", &task.prompt)?;
    let response_path = write_artifact(&artifacts, "response.txt", &content)?;
    let raw_response_path = match raw_response {
        Some(raw) => Some(write_artifact(&artifacts, "raw-response.json", &raw)?),
        None => None,
    };

    let mut metrics = serde_json::Map::new();
    metrics.insert("wall_time_ms".into(), serde_json::json!(wall_time_ms));
    metrics.insert("setup_time_ms".into(), serde_json::json!(setup_time_ms));
    metrics.insert("target_time_ms".into(), serde_json::json!(target_time_ms));
    metrics.insert("response_chars".into(), serde_json::json!(content.len()));
    for (key, value) in &provider_metrics {
        metrics.insert(key.clone(), value.clone());
    }
    let output_tokens_per_second =
        estimate_output_tokens_per_second(&provider_metrics, wall_time_ms);
    if let Some(value) = output_tokens_per_second {
        metrics.insert("output_tokens_per_second".into(), serde_json::json!(value));
    }
    let cost_usd = estimate_cost_usd(
        &target.kind,
        &target.adapter_id,
        &target_config,
        &provider_metrics,
    );
    let pricing_assumptions = cache_pricing_fallback_assumptions(&target_config, &provider_metrics);
    let warnings = pricing_assumption_warnings(&pricing_assumptions);
    if let Some(cost) = cost_usd {
        metrics.insert("cost_usd".into(), serde_json::json!(cost));
    }
    insert_pricing_assumption_metrics(&mut metrics, &pricing_assumptions);
    insert_v1_metric_aliases(&mut metrics, &prompt_score.status, Some(prompt_score.score));

    let result_json = serde_json::json!({
        "run_id": run_id,
        "target_id": target.id,
        "task_id": task.id,
        "task_weight": normalized_task_weight(task.weight),
        "status": prompt_score.status,
        "score": prompt_score.score,
        "error_code": error_code.clone(),
        "error_message": prompt_score.error_message.clone(),
        "warnings": warnings.clone(),
        "metrics": metrics,
        "tests": prompt_score.tests,
        "artifacts": [
            {"kind": "prompt", "path": prompt_path.to_string_lossy()},
            {"kind": "response", "path": response_path.to_string_lossy()}
        ]
    });
    let result_path = write_artifact(
        &artifacts,
        "result.json",
        &serde_json::to_string_pretty(&result_json).map_err(|err| err.to_string())?,
    )?;

    let finished_at = store::now();
    let benchmark_pack = benchmark_pack_reproducibility(pack)?;
    let prompts = prompt_reproducibility(
        optional_prompt(&task.prompt),
        Some(BENCHMARK_PROMPT_SYSTEM),
        Some(&task.prompt),
        None,
    );
    let reproducibility = serde_json::json!({
        "benchforge_version": env!("CARGO_PKG_VERSION"),
        "benchmark_pack": benchmark_pack,
        "task": {"id": task.id, "version": task.version, "weight": normalized_task_weight(task.weight), "checksum": checksum_file(&task.source_path)?},
        "target": target_reproducibility(target, &target_config),
        "prompts": prompts,
        "generation": generation_settings(&target_config, 512),
        "run": {"warmup_runs": warmup_runs, "concurrency": concurrency},
        "sandbox": "none",
        "sandbox_level": 0,
        "permission_mode": "safe-readonly",
        "network": "host",
        "scoring": {
            "expect_exact": task.scoring.expect_exact.clone(),
            "expect_contains": task.scoring.expect_contains.clone(),
            "expect_regex": task.scoring.expect_regex.clone(),
            "expect_not_contains": task.scoring.expect_not_contains.clone(),
            "expect_json": task.scoring.expect_json,
            "json_field_equals": task.scoring.json_field_equals.clone(),
            "json_field_contains": task.scoring.json_field_contains.clone(),
            "json_field_object_keys_exact": task.scoring.json_field_object_keys_exact.clone(),
            "json_field_array_exact": task.scoring.json_field_array_exact.clone(),
            "json_field_array_exact_ordered": task.scoring.json_field_array_exact_ordered.clone(),
            "json_field_number_close": task.scoring.json_field_number_close.clone(),
            "json_field_number_bounds": task.scoring.json_field_number_bounds.clone()
        },
        "host": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "host_profile": host_reproducibility(),
    });

    store::insert_run_with_group(
        conn,
        &run_id,
        run_group_id,
        &target.id,
        &pack.id,
        &task.id,
        &prompt_score.status,
        &started_at,
        &finished_at,
        error_code.as_deref(),
        prompt_score.error_message.as_deref(),
        &serde_json::json!({
            "docker": false,
            "task_type": "prompt",
            "warmup_runs": warmup_runs,
            "concurrency": concurrency
        }),
        &reproducibility,
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "score",
        Some(prompt_score.score),
        None,
        "prompt",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "wall_time_ms",
        Some(wall_time_ms as f64),
        Some("ms"),
        "prompt",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "setup_time_ms",
        Some(setup_time_ms as f64),
        Some("ms"),
        "setup",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "target_time_ms",
        Some(target_time_ms as f64),
        Some("ms"),
        "target",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "response_chars",
        Some(content.len() as f64),
        Some("chars"),
        "prompt",
    )
    .map_err(|err| err.to_string())?;
    insert_provider_metrics(conn, &run_id, &provider_metrics)?;
    if let Some(value) = output_tokens_per_second {
        store::insert_metric(
            conn,
            &run_id,
            "output_tokens_per_second",
            Some(value),
            Some("tokens/s"),
            "provider",
        )
        .map_err(|err| err.to_string())?;
    }
    if let Some(cost) = cost_usd {
        store::insert_metric(
            conn,
            &run_id,
            "cost_usd",
            Some(cost),
            Some("USD"),
            "pricing",
        )
        .map_err(|err| err.to_string())?;
    }
    insert_pricing_assumption_store_metrics(conn, &run_id, &pricing_assumptions)?;

    let mut artifact_paths = vec![
        ("prompt", prompt_path),
        ("response", response_path),
        ("result_json", result_path),
    ];
    if let Some(path) = raw_response_path {
        artifact_paths.push(("raw_response", path));
    }
    for (kind, path) in &artifact_paths {
        let digest = checksum_file(path).ok();
        store::insert_artifact(
            conn,
            &run_id,
            kind,
            path,
            Some("text/plain"),
            digest.as_deref(),
            &serde_json::json!({}),
        )
        .map_err(|err| err.to_string())?;
    }

    Ok(RunResultDto {
        id: run_id,
        target_id: target.id.clone(),
        benchmark_pack_id: pack.id.clone(),
        task_id: task.id.clone(),
        status: prompt_score.status,
        score: Some(prompt_score.score),
        wall_time_ms,
        artifacts: artifact_paths
            .iter()
            .filter_map(|(_, path)| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .collect(),
        warnings,
        error: prompt_score.error_message,
    })
}

fn run_task(
    conn: &Connection,
    target: &store::TargetRecord,
    pack: &BenchmarkPackSpec,
    task: &TaskSpec,
    docker: bool,
    warmup_runs: u32,
    concurrency: u32,
    run_group_id: Option<&str>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<RunResultDto, String> {
    ensure_not_cancelled(is_cancelled)?;
    if task.task_type == "prompt" {
        return run_prompt_task(
            conn,
            target,
            pack,
            task,
            warmup_runs,
            concurrency,
            run_group_id,
            is_cancelled,
        );
    }
    if task.task_type == "benchmark_harness" {
        return run_benchmark_harness_task(conn, target, pack, task, run_group_id, is_cancelled);
    }
    let setup_started = Instant::now();
    let started_at = store::now();
    let run_id = create_run_id();
    let run_dir = paths::runs_dir().join(&run_id);
    let workspace = run_dir.join("workspace");
    let artifacts = run_dir.join("artifacts");
    fs::create_dir_all(&workspace).map_err(|err| err.to_string())?;
    fs::create_dir_all(&artifacts).map_err(|err| err.to_string())?;

    let mut warnings = Vec::new();
    let target_config: serde_json::Value =
        serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));
    let fixture = resolve_fixture(task)?;
    copy_dir(&fixture, &workspace)?;
    init_git(&workspace, is_cancelled)?;
    let baseline_commit = git_rev_parse(&workspace, "HEAD", is_cancelled)?;
    let baseline_tree = git_rev_parse(&workspace, "HEAD^{tree}", is_cancelled)?;
    ensure_not_cancelled(is_cancelled)?;
    let setup_time_ms = setup_started.elapsed().as_millis() as u64;

    let mut model_output = None;
    let mut model_call_wall_time_ms = None;
    let mut model_system_prompt = None;
    let mut model_prompt = None;
    let mut cli_agent_run = None;
    let mut execution_error = None;
    let target_started = Instant::now();
    let execution = match target.kind.as_str() {
        "mock" => run_mock_target(task, &workspace).map(|_| None),
        "cli_agent" => {
            match run_cli_agent(target, &target_config, task, &workspace, is_cancelled) {
                Ok(run) => {
                    let error = cli_agent_execution_error(&run.capture);
                    cli_agent_run = Some(run);
                    match error {
                        Some(err) => Err(err),
                        None => Ok(None),
                    }
                }
                Err(err) => Err(err),
            }
        }
        "direct_model" | "harnessed_model" => {
            let request = ModelClientRequest::code_edit(task, &workspace);
            model_system_prompt = Some(request.system_prompt.to_string());
            model_prompt = Some(request.user_prompt.to_string());
            let model_call_started = Instant::now();
            let result = run_direct_model_target(target, &request, &workspace, is_cancelled);
            model_call_wall_time_ms = Some(model_call_started.elapsed().as_millis() as u64);
            match result {
                Ok(output) => Ok(Some(output)),
                Err(err) => {
                    if let Some(output) = err.output {
                        model_output = Some(output);
                    }
                    Err(err.message)
                }
            }
        }
        other => Err(format!("unsupported target kind: {}", other)),
    };
    let target_time_ms = target_started.elapsed().as_millis() as u64;

    match execution {
        Ok(output) => model_output = output,
        Err(err) => {
            execution_error = Some(err.clone());
            warnings.push(err);
        }
    }

    ensure_not_cancelled(is_cancelled)?;
    let score_started = Instant::now();
    let mut docker_scoring_image = None;
    let mut scoring_command_metadata = None;
    let using_docker_scoring = uses_docker_scoring(docker, task);
    let scoring = if using_docker_scoring {
        match run_scoring_docker(task, &workspace, &artifacts, is_cancelled) {
            Ok((capture, image_metadata, command_metadata)) => {
                docker_scoring_image = Some(image_metadata);
                scoring_command_metadata = Some(command_metadata);
                Ok(capture)
            }
            Err(err) => Err(err),
        }
    } else {
        match run_scoring_host(task, &workspace, is_cancelled) {
            Ok((capture, command_metadata)) => {
                scoring_command_metadata = Some(command_metadata);
                Ok(capture)
            }
            Err(err) => Err(err),
        }
    };
    let score_elapsed = score_started.elapsed().as_millis() as u64;

    let diff = run_git_diff(&workspace, is_cancelled)
        .unwrap_or_else(|err| format!("failed to capture diff: {}", err));
    let diff_sha256 = checksum_text(&diff);
    let diff_stats = diff_change_stats(&diff);
    let diff_path = write_artifact(&artifacts, "diff.patch", &diff)?;

    let mut status = "passed".to_string();
    let mut score = Some(1.0);
    let mut error_code = None;
    let mut error_message = None;
    let command_capture = match scoring {
        Ok(capture) => capture,
        Err(err) => {
            status = "error".into();
            score = Some(0.0);
            error_code = Some("scoring_failed".to_string());
            error_message = Some(err.clone());
            CommandCapture {
                stdout: String::new(),
                stderr: err,
                code: None,
                timed_out: false,
                wall_time_ms: score_elapsed,
                peak_rss_mb: None,
            }
        }
    };

    if command_capture.timed_out {
        status = "timeout".into();
        score = Some(0.0);
        error_code = Some("timeout".into());
        error_message = Some("scoring command timed out".into());
    } else if command_capture.code.unwrap_or(1) != 0 {
        status = "failed".into();
        score = Some(0.0);
        error_code.get_or_insert_with(|| "test_failed".into());
    }

    let secret_values = collect_secret_values();
    let secret_leak_hits = safety::detect_secret_leaks(
        &[
            ("stdout", &command_capture.stdout),
            ("stderr", &command_capture.stderr),
        ],
        &secret_values,
    );
    let stdout = safety::redact_secrets(&command_capture.stdout, &secret_values);
    let stderr = safety::redact_secrets(&command_capture.stderr, &secret_values);
    let mut dangerous_command_hits = safety::detect_suspicious_commands(&stdout);
    dangerous_command_hits.extend(safety::detect_suspicious_commands(&stderr));
    dangerous_command_hits.sort();
    dangerous_command_hits.dedup();
    let dangerous_command_hit_count = dangerous_command_hits.len();
    let commands_observed_count = repo_code_commands_observed_count(target, task);
    warnings.extend(dangerous_command_hits.clone());
    if !secret_leak_hits.is_empty() {
        warnings.push("secret_leak_detected".into());
    }
    let execution_error_code = execution_error
        .as_deref()
        .map(|err| target_execution_error_code(target, err).to_string());
    let execution_error = execution_error.map(|err| safety::redact_secrets(&err, &secret_values));
    warnings = warnings
        .into_iter()
        .map(|warning| safety::redact_secrets(&warning, &secret_values))
        .collect();
    if let Some(err) = &execution_error {
        status = "error".into();
        score = Some(0.0);
        error_code = execution_error_code;
        error_message = Some(err.clone());
    }

    let stdout_path = write_artifact(&artifacts, "stdout.txt", &stdout)?;
    let stderr_path = write_artifact(&artifacts, "stderr.txt", &stderr)?;
    let prompts = prompt_reproducibility(
        optional_prompt(&task.prompt),
        model_system_prompt.as_deref(),
        model_prompt.as_deref(),
        None,
    );
    let model_system_prompt_path = match model_system_prompt {
        Some(prompt) => Some(write_artifact(
            &artifacts,
            "model-system-prompt.txt",
            &safety::redact_secrets(&prompt, &secret_values),
        )?),
        None => None,
    };
    let model_prompt_path = match model_prompt {
        Some(prompt) => Some(write_artifact(
            &artifacts,
            "model-prompt.txt",
            &safety::redact_secrets(&prompt, &secret_values),
        )?),
        None => None,
    };
    let mut provider_metrics = serde_json::Map::new();
    let mut model_output_path = None;
    let mut raw_response_path = None;
    if let Some(err) = execution_error
        .as_deref()
        .filter(|_| matches!(target.kind.as_str(), "direct_model" | "harnessed_model"))
    {
        provider_metrics = provider_error_transport_metrics(err);
    }
    if let Some(output) = model_output {
        provider_metrics = output.metrics;
        let redacted_output = safety::redact_secrets(&output.content, &secret_values);
        model_output_path = Some(write_artifact(
            &artifacts,
            "model-output.txt",
            &redacted_output,
        )?);
        if let Some(raw) = output.raw_response {
            raw_response_path = Some(write_artifact(
                &artifacts,
                "raw-response.json",
                &safety::redact_secrets(&raw, &secret_values),
            )?);
        }
    }
    let cli_agent_artifacts = match &cli_agent_run {
        Some(run) => Some(write_cli_agent_evidence_artifacts(
            &artifacts,
            run,
            &task.prompt,
            &secret_values,
        )?),
        None => None,
    };
    ensure_provider_model_metric(&mut provider_metrics, &target_config);
    if let Some(value) = model_call_wall_time_ms {
        provider_metrics
            .entry("model_call_wall_time_ms")
            .or_insert_with(|| serde_json::json!(value));
    }
    let output_tokens_per_second = model_call_wall_time_ms.and_then(|wall_time_ms| {
        estimate_output_tokens_per_second(&provider_metrics, wall_time_ms)
    });
    let cost_usd = estimate_cost_usd(
        &target.kind,
        &target.adapter_id,
        &target_config,
        &provider_metrics,
    );
    let pricing_assumptions = cache_pricing_fallback_assumptions(&target_config, &provider_metrics);
    warnings.extend(pricing_assumption_warnings(&pricing_assumptions));
    let mut result_metrics = serde_json::Map::from_iter([
        (
            "wall_time_ms".to_string(),
            serde_json::json!(command_capture.wall_time_ms),
        ),
        (
            "setup_time_ms".to_string(),
            serde_json::json!(setup_time_ms),
        ),
        (
            "target_time_ms".to_string(),
            serde_json::json!(target_time_ms),
        ),
        (
            "evaluation_time_ms".to_string(),
            serde_json::json!(score_elapsed),
        ),
        (
            "exit_code".to_string(),
            serde_json::json!(command_capture.code),
        ),
        ("stdout_bytes".to_string(), serde_json::json!(stdout.len())),
        ("stderr_bytes".to_string(), serde_json::json!(stderr.len())),
        (
            "files_changed".to_string(),
            serde_json::json!(diff_stats.files_changed),
        ),
        (
            "lines_added".to_string(),
            serde_json::json!(diff_stats.lines_added),
        ),
        (
            "lines_deleted".to_string(),
            serde_json::json!(diff_stats.lines_deleted),
        ),
        (
            "commands_observed_count".to_string(),
            serde_json::json!(commands_observed_count),
        ),
        (
            "dangerous_command_hits".to_string(),
            serde_json::json!(dangerous_command_hit_count),
        ),
    ]);
    if let Some(value) = command_capture.peak_rss_mb {
        result_metrics.insert("peak_rss_mb".to_string(), serde_json::json!(value));
    }
    if let Some(run) = &cli_agent_run {
        result_metrics.insert(
            "cli_agent_exit_code".into(),
            serde_json::json!(run.capture.code),
        );
        result_metrics.insert(
            "cli_agent_timed_out".into(),
            serde_json::json!(run.capture.timed_out),
        );
        result_metrics.insert(
            "cli_agent_wall_time_ms".into(),
            serde_json::json!(run.capture.wall_time_ms),
        );
        if let Some(artifacts) = &cli_agent_artifacts {
            result_metrics.insert(
                "cli_agent_stdout_bytes".into(),
                serde_json::json!(artifacts.stdout_bytes),
            );
            result_metrics.insert(
                "cli_agent_stderr_bytes".into(),
                serde_json::json!(artifacts.stderr_bytes),
            );
        }
        if let Some(value) = run.capture.peak_rss_mb {
            result_metrics.insert("cli_agent_peak_rss_mb".into(), serde_json::json!(value));
        }
    }
    for (key, value) in &provider_metrics {
        result_metrics.insert(key.clone(), value.clone());
    }
    if let Some(value) = output_tokens_per_second {
        result_metrics.insert("output_tokens_per_second".into(), serde_json::json!(value));
    }
    if let Some(cost) = cost_usd {
        result_metrics.insert("cost_usd".into(), serde_json::json!(cost));
    }
    insert_pricing_assumption_metrics(&mut result_metrics, &pricing_assumptions);
    insert_v1_metric_aliases(&mut result_metrics, &status, score);
    let mut result_artifacts = vec![
        serde_json::json!({"kind": "stdout", "path": stdout_path.to_string_lossy()}),
        serde_json::json!({"kind": "stderr", "path": stderr_path.to_string_lossy()}),
        serde_json::json!({"kind": "git_diff", "path": diff_path.to_string_lossy()}),
    ];
    if let Some(path) = &model_system_prompt_path {
        result_artifacts.push(
            serde_json::json!({"kind": "model_system_prompt", "path": path.to_string_lossy()}),
        );
    }
    if let Some(path) = &model_prompt_path {
        result_artifacts
            .push(serde_json::json!({"kind": "model_prompt", "path": path.to_string_lossy()}));
    }
    if let Some(path) = &model_output_path {
        result_artifacts
            .push(serde_json::json!({"kind": "model_output", "path": path.to_string_lossy()}));
    }
    if let Some(path) = &raw_response_path {
        result_artifacts
            .push(serde_json::json!({"kind": "raw_response", "path": path.to_string_lossy()}));
    }
    if let Some(artifacts) = &cli_agent_artifacts {
        result_artifacts.push(
            serde_json::json!({"kind": "cli_stdout", "path": artifacts.stdout_path.to_string_lossy()}),
        );
        result_artifacts.push(
            serde_json::json!({"kind": "cli_stderr", "path": artifacts.stderr_path.to_string_lossy()}),
        );
        result_artifacts.push(
            serde_json::json!({"kind": "cli_agent_command", "path": artifacts.command_path.to_string_lossy()}),
        );
    }
    let docker_image_path = match &docker_scoring_image {
        Some(metadata) => Some(write_artifact(
            &artifacts,
            "docker-image.json",
            &serde_json::to_string_pretty(metadata).map_err(|err| err.to_string())?,
        )?),
        None => None,
    };
    let scoring_command_metadata_path = match &scoring_command_metadata {
        Some(metadata) => Some(write_artifact(
            &artifacts,
            "scoring-command.json",
            &serde_json::to_string_pretty(metadata).map_err(|err| err.to_string())?,
        )?),
        None => None,
    };
    if let Some(path) = &scoring_command_metadata_path {
        result_artifacts
            .push(serde_json::json!({"kind": "scoring_command", "path": path.to_string_lossy()}));
    }
    let result_json = serde_json::json!({
        "run_id": run_id,
        "target_id": target.id,
        "task_id": task.id,
        "status": status,
        "score": score,
        "warnings": warnings.clone(),
        "metrics": result_metrics,
        "tests": parse_test_summary(&stdout, &stderr, task.scoring.parse.as_deref()),
        "target_execution_error": execution_error.as_deref(),
        "artifacts": result_artifacts,
        "target_execution": {
            "cli_agent": cli_agent_artifacts.as_ref().map(|artifacts| artifacts.evidence.clone())
        },
        "safety": {
            "dangerous_command_hits": dangerous_command_hits,
            "secret_leak_hits": secret_leak_hits,
            "workspace_isolated": true,
            "sandbox_level": sandbox_level(docker, task),
            "permission_mode": permission_mode_label(docker, task),
            "scoring_environment": sandbox_environment_label(docker, task),
            "docker_image": &docker_scoring_image,
            "docker_resource_limits": if using_docker_scoring { docker_scoring_resource_limits() } else { serde_json::Value::Null },
            "scoring_command": &scoring_command_metadata
        }
    });
    let result_path = write_artifact(
        &artifacts,
        "result.json",
        &serde_json::to_string_pretty(&result_json).map_err(|err| err.to_string())?,
    )?;

    let finished_at = store::now();
    let benchmark_pack = benchmark_pack_reproducibility(pack)?;
    let mut reproducibility = serde_json::json!({
        "benchforge_version": env!("CARGO_PKG_VERSION"),
        "benchmark_pack": benchmark_pack,
        "task": {"id": task.id, "version": task.version, "weight": normalized_task_weight(task.weight), "checksum": checksum_file(&task.source_path)?},
        "target": target_reproducibility(target, &target_config),
        "prompts": prompts,
        "generation": generation_settings(&target_config, 4096),
        "run": {"warmup_runs": warmup_runs, "concurrency": concurrency},
        "sandbox": if using_docker_scoring { "docker" } else { "isolated-workspace" },
        "sandbox_level": sandbox_level(docker, task),
        "permission_mode": permission_mode_label(docker, task),
        "network": if using_docker_scoring { "none" } else { "host" },
        "environment": sandbox_environment_label(docker, task),
        "workspace_path": workspace.to_string_lossy(),
        "workspace": {
            "path": workspace.to_string_lossy(),
            "git": {
                "baseline_commit": baseline_commit,
                "baseline_tree": baseline_tree,
                "diff_sha256": diff_sha256,
                "diff_includes_untracked": true,
                "diff_excluded_paths": git_diff_excluded_pathspecs()
            }
        },
        "scoring_command": task.scoring.command,
        "scoring_command_metadata": &scoring_command_metadata,
        "host": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "host_profile": host_reproducibility(),
    });
    if let Some(metadata) = &docker_scoring_image {
        reproducibility["docker"] = serde_json::json!({
            "scoring_image": metadata,
            "resource_limits": docker_scoring_resource_limits()
        });
    }
    if let Some(artifacts) = &cli_agent_artifacts {
        reproducibility["cli_agent"] = artifacts.evidence.clone();
    }

    store::insert_run_with_group(
        conn,
        &run_id,
        run_group_id,
        &target.id,
        &pack.id,
        &task.id,
        &status,
        &started_at,
        &finished_at,
        error_code.as_deref(),
        error_message.as_deref(),
        &serde_json::json!({
            "docker": docker,
            "warmup_runs": warmup_runs,
            "concurrency": concurrency
        }),
        &reproducibility,
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(conn, &run_id, "score", score, None, "scoring")
        .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "wall_time_ms",
        Some(command_capture.wall_time_ms as f64),
        Some("ms"),
        "scoring",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "setup_time_ms",
        Some(setup_time_ms as f64),
        Some("ms"),
        "setup",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "target_time_ms",
        Some(target_time_ms as f64),
        Some("ms"),
        "target",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "evaluation_time_ms",
        Some(score_elapsed as f64),
        Some("ms"),
        "scoring",
    )
    .map_err(|err| err.to_string())?;
    if let Some(code) = command_capture.code {
        store::insert_metric(
            conn,
            &run_id,
            "exit_code",
            Some(code as f64),
            None,
            "scoring",
        )
        .map_err(|err| err.to_string())?;
    }
    store::insert_metric(
        conn,
        &run_id,
        "stdout_bytes",
        Some(stdout.len() as f64),
        Some("bytes"),
        "scoring",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "stderr_bytes",
        Some(stderr.len() as f64),
        Some("bytes"),
        "scoring",
    )
    .map_err(|err| err.to_string())?;
    if let Some(value) = command_capture.peak_rss_mb {
        store::insert_metric(
            conn,
            &run_id,
            "peak_rss_mb",
            Some(value),
            Some("MB"),
            "process",
        )
        .map_err(|err| err.to_string())?;
    }
    store::insert_metric(
        conn,
        &run_id,
        "files_changed",
        Some(diff_stats.files_changed as f64),
        None,
        "workspace",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "lines_added",
        Some(diff_stats.lines_added as f64),
        Some("lines"),
        "workspace",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "lines_deleted",
        Some(diff_stats.lines_deleted as f64),
        Some("lines"),
        "workspace",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "commands_observed_count",
        Some(commands_observed_count as f64),
        None,
        "process",
    )
    .map_err(|err| err.to_string())?;
    store::insert_metric(
        conn,
        &run_id,
        "dangerous_command_hits",
        Some(dangerous_command_hit_count as f64),
        None,
        "safety",
    )
    .map_err(|err| err.to_string())?;
    if let Some(run) = &cli_agent_run {
        if let Some(code) = run.capture.code {
            store::insert_metric(
                conn,
                &run_id,
                "cli_agent_exit_code",
                Some(code as f64),
                None,
                "target",
            )
            .map_err(|err| err.to_string())?;
        }
        store::insert_metric(
            conn,
            &run_id,
            "cli_agent_timed_out",
            Some(if run.capture.timed_out { 1.0 } else { 0.0 }),
            None,
            "target",
        )
        .map_err(|err| err.to_string())?;
        store::insert_metric(
            conn,
            &run_id,
            "cli_agent_wall_time_ms",
            Some(run.capture.wall_time_ms as f64),
            Some("ms"),
            "target",
        )
        .map_err(|err| err.to_string())?;
        if let Some(artifacts) = &cli_agent_artifacts {
            store::insert_metric(
                conn,
                &run_id,
                "cli_agent_stdout_bytes",
                Some(artifacts.stdout_bytes as f64),
                Some("bytes"),
                "target",
            )
            .map_err(|err| err.to_string())?;
            store::insert_metric(
                conn,
                &run_id,
                "cli_agent_stderr_bytes",
                Some(artifacts.stderr_bytes as f64),
                Some("bytes"),
                "target",
            )
            .map_err(|err| err.to_string())?;
        }
        if let Some(value) = run.capture.peak_rss_mb {
            store::insert_metric(
                conn,
                &run_id,
                "cli_agent_peak_rss_mb",
                Some(value),
                Some("MB"),
                "target",
            )
            .map_err(|err| err.to_string())?;
        }
    }
    insert_provider_metrics(conn, &run_id, &provider_metrics)?;
    if let Some(value) = output_tokens_per_second {
        store::insert_metric(
            conn,
            &run_id,
            "output_tokens_per_second",
            Some(value),
            Some("tokens/s"),
            "provider",
        )
        .map_err(|err| err.to_string())?;
    }
    if let Some(cost) = cost_usd {
        store::insert_metric(
            conn,
            &run_id,
            "cost_usd",
            Some(cost),
            Some("USD"),
            "pricing",
        )
        .map_err(|err| err.to_string())?;
    }
    insert_pricing_assumption_store_metrics(conn, &run_id, &pricing_assumptions)?;
    let mut artifact_names = vec![
        "stdout.txt".into(),
        "stderr.txt".into(),
        "diff.patch".into(),
        "result.json".into(),
    ];
    if model_system_prompt_path.is_some() {
        artifact_names.push("model-system-prompt.txt".into());
    }
    if model_prompt_path.is_some() {
        artifact_names.push("model-prompt.txt".into());
    }
    if model_output_path.is_some() {
        artifact_names.push("model-output.txt".into());
    }
    if raw_response_path.is_some() {
        artifact_names.push("raw-response.json".into());
    }
    if cli_agent_artifacts.is_some() {
        artifact_names.push("cli-stdout.txt".into());
        artifact_names.push("cli-stderr.txt".into());
        artifact_names.push("cli-agent-command.json".into());
    }
    if docker_image_path.is_some() {
        artifact_names.push("docker-image.json".into());
    }
    if scoring_command_metadata_path.is_some() {
        artifact_names.push("scoring-command.json".into());
    }
    let mut artifact_paths = vec![
        ("stdout", stdout_path),
        ("stderr", stderr_path),
        ("git_diff", diff_path),
        ("result_json", result_path),
    ];
    if let Some(path) = model_system_prompt_path {
        artifact_paths.push(("model_system_prompt", path));
    }
    if let Some(path) = model_prompt_path {
        artifact_paths.push(("model_prompt", path));
    }
    if let Some(path) = model_output_path {
        artifact_paths.push(("model_output", path));
    }
    if let Some(path) = raw_response_path {
        artifact_paths.push(("raw_response", path));
    }
    if let Some(artifacts) = cli_agent_artifacts {
        artifact_paths.push(("cli_stdout", artifacts.stdout_path));
        artifact_paths.push(("cli_stderr", artifacts.stderr_path));
        artifact_paths.push(("cli_agent_command", artifacts.command_path));
    }
    if let Some(path) = docker_image_path {
        artifact_paths.push(("docker_image", path));
    }
    if let Some(path) = scoring_command_metadata_path {
        artifact_paths.push(("scoring_command", path));
    }
    for (kind, path) in artifact_paths {
        let digest = checksum_file(&path).ok();
        store::insert_artifact(
            conn,
            &run_id,
            kind,
            &path,
            Some("text/plain"),
            digest.as_deref(),
            &serde_json::json!({}),
        )
        .map_err(|err| err.to_string())?;
    }

    Ok(RunResultDto {
        id: run_id,
        target_id: target.id.clone(),
        benchmark_pack_id: pack.id.clone(),
        task_id: task.id.clone(),
        status,
        score,
        wall_time_ms: command_capture.wall_time_ms,
        artifacts: artifact_names,
        warnings,
        error: error_message,
    })
}

pub fn load_pack(id: &str) -> Result<BenchmarkPackSpec, String> {
    let discovered = find_benchmark_pack_path(id, &benchmark_pack_roots())?;
    load_pack_from_path_with_source(&discovered.path, discovered.source)
}

fn find_benchmark_pack_path(
    id: &str,
    roots: &[BenchmarkPackRoot],
) -> Result<DiscoveredBenchmarkPack, String> {
    validate_benchmark_pack_id(id)?;
    let mut found: Option<DiscoveredBenchmarkPack> = None;
    for discovered in discover_benchmark_pack_paths(roots)? {
        let pack = match load_pack_from_path_with_source(&discovered.path, discovered.source) {
            Ok(pack) => pack,
            Err(err) => {
                if discovered_pack_dir_name(&discovered.path).as_deref() == Some(id) {
                    return Err(err);
                }
                continue;
            }
        };
        if pack.id != id {
            continue;
        }
        if let Some(previous) = found {
            return Err(format!(
                "duplicate benchmark pack id {} at {} and {}",
                id,
                previous.path.display(),
                discovered.path.display()
            ));
        }
        found = Some(discovered);
    }
    found.ok_or_else(|| format!("benchmark pack {} not found", id))
}

fn discovered_pack_dir_name(path: &Path) -> Option<String> {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn load_pack_from_path_with_source(
    path: &Path,
    source: &'static str,
) -> Result<BenchmarkPackSpec, String> {
    let raw = fs::read_to_string(path).map_err(|err| format!("{}: {}", path.display(), err))?;
    let mut pack: BenchmarkPackSpec =
        serde_yaml::from_str(&raw).map_err(|err| format!("{}: {}", path.display(), err))?;
    validate_benchmark_pack_id(&pack.id)?;
    let pack_path = path
        .canonicalize()
        .map_err(|err| format!("{}: {}", path.display(), err))?;
    let pack_dir = pack_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", pack_path.display()))?
        .to_path_buf();
    pack.pack_path = pack_path;
    pack.pack_dir = pack_dir;
    pack.source = source.to_string();
    Ok(pack)
}

pub fn load_tasks(pack: &BenchmarkPackSpec) -> Result<Vec<TaskSpec>, String> {
    let pack_dir = pack_dir(pack);
    pack.tasks
        .iter()
        .map(|task_path| {
            let source_path = resolve_pack_relative_path(&pack_dir, task_path, "task")?;
            let raw = fs::read_to_string(&source_path)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
            let mut task: TaskSpec = serde_yaml::from_str(&raw)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
            task.source_path = source_path;
            Ok(task)
        })
        .collect()
}

pub fn select_tasks_for_run(
    tasks: Vec<TaskSpec>,
    requested_task_ids: &[String],
) -> Result<Vec<TaskSpec>, String> {
    if requested_task_ids.is_empty() {
        return Ok(tasks);
    }

    let mut requested = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    let mut blanks = 0_usize;
    for task_id in requested_task_ids {
        let task_id = task_id.trim();
        if task_id.is_empty() {
            blanks += 1;
            continue;
        }
        if !requested.insert(task_id.to_string()) {
            duplicates.insert(task_id.to_string());
        }
    }
    if blanks > 0 {
        return Err("task_filter_invalid: taskIds cannot contain blank IDs".into());
    }
    if !duplicates.is_empty() {
        return Err(format!(
            "task_filter_invalid: duplicate taskIds requested: {}",
            duplicates.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let selected = tasks
        .into_iter()
        .filter(|task| requested.contains(&task.id))
        .collect::<Vec<_>>();
    let selected_ids = selected
        .iter()
        .map(|task| task.id.clone())
        .collect::<BTreeSet<_>>();
    let missing = requested
        .difference(&selected_ids)
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "task_filter_invalid: requested taskIds are not in this pack: {}",
            missing.join(", ")
        ));
    }
    Ok(selected)
}

fn pack_dir(pack: &BenchmarkPackSpec) -> PathBuf {
    if pack.pack_dir.as_os_str().is_empty() {
        builtin_benchmark_pack_root().join(&pack.id)
    } else {
        pack.pack_dir.clone()
    }
}

fn pack_file_path(pack: &BenchmarkPackSpec) -> PathBuf {
    if pack.pack_path.as_os_str().is_empty() {
        builtin_benchmark_pack_root()
            .join(&pack.id)
            .join("pack.yaml")
    } else {
        pack.pack_path.clone()
    }
}

fn validate_benchmark_pack_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!(
            "invalid benchmark pack id {}; use only ASCII letters, numbers, '-' or '_'",
            id
        ));
    }
    Ok(())
}

fn resolve_pack_relative_path(
    pack_dir: &Path,
    relative_path: &str,
    label: &str,
) -> Result<PathBuf, String> {
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "{} path must stay inside the benchmark pack: {}",
            label, relative_path
        ));
    }
    let canonical_pack_dir = pack_dir
        .canonicalize()
        .map_err(|err| format!("{}: {}", pack_dir.display(), err))?;
    let candidate = pack_dir.join(relative);
    let source_path = candidate
        .canonicalize()
        .map_err(|err| format!("{}: {}", candidate.display(), err))?;
    if !source_path.starts_with(&canonical_pack_dir) {
        return Err(format!(
            "{} path {} resolves outside benchmark pack {}",
            label,
            source_path.display(),
            canonical_pack_dir.display()
        ));
    }
    Ok(source_path)
}

fn resolve_fixture(task: &TaskSpec) -> Result<PathBuf, String> {
    let Some(fixture) = &task.fixture else {
        return Err(format!("task {} has no fixture", task.id));
    };
    if Path::new(fixture).is_absolute() {
        return Err(format!(
            "task {} fixture path must be relative, got {}",
            task.id, fixture
        ));
    }
    let base = task
        .source_path
        .parent()
        .ok_or_else(|| "task source has no parent".to_string())?;
    let path = base.join(fixture);
    let fixture_path = path
        .canonicalize()
        .map_err(|err| format!("{}: {}", path.display(), err))?;
    let allowed_roots = allowed_fixture_roots(task)?;
    if allowed_roots
        .iter()
        .any(|root| fixture_path.starts_with(root))
    {
        Ok(fixture_path)
    } else {
        Err(format!(
            "task {} fixture {} is outside allowed fixture roots: {}",
            task.id,
            fixture_path.display(),
            allowed_roots
                .iter()
                .map(|root| root.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

fn allowed_fixture_roots(task: &TaskSpec) -> Result<Vec<PathBuf>, String> {
    let repo_root = paths::resource_root()
        .canonicalize()
        .map_err(|err| format!("{}: {}", paths::resource_root().display(), err))?;
    let mut roots = Vec::new();
    let repo_fixtures = repo_root.join("fixtures");
    if repo_fixtures.exists() {
        roots.push(
            repo_fixtures
                .canonicalize()
                .map_err(|err| format!("{}: {}", repo_fixtures.display(), err))?,
        );
    }

    let source_path = task
        .source_path
        .canonicalize()
        .map_err(|err| format!("{}: {}", task.source_path.display(), err))?;
    if let Some(pack_dir) = pack_dir_for_task_source(&source_path) {
        let pack_fixtures = pack_dir.join("fixtures");
        if pack_fixtures.exists() {
            roots.push(
                pack_fixtures
                    .canonicalize()
                    .map_err(|err| format!("{}: {}", pack_fixtures.display(), err))?,
            );
        }
    }

    #[cfg(test)]
    if !source_path.starts_with(&repo_root) {
        if let Some(parent) = source_path.parent() {
            roots.push(
                parent
                    .canonicalize()
                    .map_err(|err| format!("{}: {}", parent.display(), err))?,
            );
        }
    }

    roots.sort();
    roots.dedup();
    if roots.is_empty() {
        Err(format!(
            "task {} has no allowed fixture roots for source {}",
            task.id,
            task.source_path.display()
        ))
    } else {
        Ok(roots)
    }
}

fn pack_dir_for_task_source(source_path: &Path) -> Option<PathBuf> {
    source_path
        .ancestors()
        .find(|ancestor| ancestor.join("pack.yaml").is_file())
        .map(Path::to_path_buf)
}

fn copy_dir(source: &Path, dest: &Path) -> Result<(), String> {
    let source_root = source
        .canonicalize()
        .map_err(|err| format!("{}: {}", source.display(), err))?;
    copy_dir_checked(&source_root, &source_root, dest)
}

fn copy_dir_checked(source_root: &Path, source: &Path, dest: &Path) -> Result<(), String> {
    let source = source
        .canonicalize()
        .map_err(|err| format!("{}: {}", source.display(), err))?;
    if !source.starts_with(source_root) {
        return Err(format!(
            "fixture copy attempted to leave source root: {}",
            source.display()
        ));
    }
    fs::create_dir_all(dest).map_err(|err| err.to_string())?;
    for entry in fs::read_dir(&source).map_err(|err| format!("{}: {}", source.display(), err))? {
        let entry = entry.map_err(|err| err.to_string())?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|err| format!("{}: {}", source_path.display(), err))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "fixture symlinks are not allowed: {}",
                source_path.display()
            ));
        }
        if metadata.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir_checked(source_root, &source_path, &dest_path)?;
        } else {
            fs::copy(&source_path, &dest_path)
                .map_err(|err| format!("{}: {}", source_path.display(), err))?;
        }
    }
    Ok(())
}

fn init_git(workspace: &Path, is_cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    run_git_command(workspace, &["init"], is_cancelled)?;
    run_git_command(workspace, &["add", "."], is_cancelled)?;
    run_git_command(
        workspace,
        &[
            "-c",
            "user.email=benchforge@example.invalid",
            "-c",
            "user.name=BenchForge",
            "commit",
            "-m",
            "baseline",
        ],
        is_cancelled,
    )?;
    Ok(())
}

fn run_git_command(
    workspace: &Path,
    args: &[&str],
    is_cancelled: &dyn Fn() -> bool,
) -> Result<CommandCapture, String> {
    let capture = run_command_capture_checked(
        sandboxed_command_in(workspace, "git", args)?,
        Duration::from_secs(30),
        is_cancelled,
    )?;
    if capture.timed_out {
        return Err(format!("git {} timed out", args.join(" ")));
    }
    if capture.code.unwrap_or(1) != 0 {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            capture.stderr.lines().next().unwrap_or("non-zero exit")
        ));
    }
    Ok(capture)
}

fn git_rev_parse(
    workspace: &Path,
    rev: &str,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    let capture = run_git_command(workspace, &["rev-parse", rev], is_cancelled)?;
    Ok(capture.stdout.trim().to_string())
}

fn git_diff_excluded_pathspecs() -> Vec<&'static str> {
    vec![
        SANDBOX_HOME_DIR,
        SANDBOX_TMP_DIR,
        SANDBOX_NPM_CACHE_DIR,
        ".benchforge-venv",
        ".benchforge-model.patch",
        ".pytest_cache",
        "__pycache__",
        "node_modules",
    ]
}

fn git_pathspec_args() -> Vec<String> {
    let mut args = vec!["--".to_string(), ".".to_string()];
    args.extend(
        git_diff_excluded_pathspecs()
            .into_iter()
            .map(|path| format!(":(exclude){}", path)),
    );
    args
}

fn git_intent_to_add_untracked(
    workspace: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let mut args = vec!["add".to_string(), "-N".to_string()];
    args.extend(git_pathspec_args());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git_command(workspace, &arg_refs, is_cancelled)?;
    Ok(())
}

fn run_mock_target(task: &TaskSpec, workspace: &Path) -> Result<(), String> {
    match task.id.as_str() {
        "python-rate-limit-001" => {
            fs::write(workspace.join("app.py"), PYTHON_RATE_LIMIT_FIX)
                .map_err(|err| err.to_string())?;
            Ok(())
        }
        "js-sanitize-filename-001" => {
            fs::write(workspace.join("upload.js"), JS_SANITIZE_FIX)
                .map_err(|err| err.to_string())?;
            Ok(())
        }
        "code-edit-python-config-merge-001" => {
            fs::write(workspace.join("config_merge.py"), PYTHON_CONFIG_MERGE_FIX)
                .map_err(|err| err.to_string())?;
            Ok(())
        }
        "code-edit-js-retry-delay-001" => {
            fs::write(workspace.join("retry.js"), JS_RETRY_DELAY_FIX)
                .map_err(|err| err.to_string())?;
            Ok(())
        }
        _ => Err(format!("mock target has no fixture fix for {}", task.id)),
    }
}

fn run_cli_agent(
    target: &store::TargetRecord,
    target_config: &serde_json::Value,
    task: &TaskSpec,
    workspace: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<CliAgentRun, String> {
    ensure_not_cancelled(is_cancelled)?;
    let adapter = cli_agent_adapter_for_target(target, target_config)?;
    run_cli_agent_with_adapter(&adapter, target_config, task, workspace, is_cancelled)
}

fn cli_agent_adapter_for_target(
    target: &store::TargetRecord,
    target_config: &serde_json::Value,
) -> Result<adapters::AdapterSpec, String> {
    if let Some(adapter) = custom_cli_agent_adapter(target, target_config)? {
        return Ok(adapter);
    }
    let Some(adapter) = adapters::find_adapter(&target.adapter_id)? else {
        return Err(format!("adapter {} not found", target.adapter_id));
    };
    Ok(adapter.spec)
}

fn custom_cli_agent_adapter(
    target: &store::TargetRecord,
    target_config: &serde_json::Value,
) -> Result<Option<adapters::AdapterSpec>, String> {
    let Some(command) = target_config
        .get("command")
        .and_then(|value| value.as_str())
    else {
        return Ok(None);
    };
    let command = command.trim();
    if command.is_empty() {
        return Err("cli_not_found: target command is empty".into());
    }
    let args = match target_config.get("args") {
        Some(value) => json_string_array(value, "args")?,
        None => Vec::new(),
    };
    let working_dir = target_config
        .get("working_dir")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let env = target_config
        .get("env")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let validation = target_config
        .get("validation")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Ok(Some(adapters::AdapterSpec {
        id: target.adapter_id.clone(),
        name: target.name.clone(),
        kind: "cli_agent".into(),
        adapter_version: 1,
        schema_version: 1,
        default_base_url: None,
        command: Some(command.to_string()),
        args,
        working_dir,
        timeout_seconds: None,
        env,
        capabilities: serde_json::json!({
            "file_editing": true,
            "repo_agent": true,
            "shell_execution": true
        }),
        security: serde_json::json!({"sandbox_required": true}),
        validation,
        metadata: serde_json::json!({"source": "target_config"}),
    }))
}

fn json_string_array(value: &serde_json::Value, field: &str) -> Result<Vec<String>, String> {
    let Some(items) = value.as_array() else {
        return Err(format!(
            "cli_agent_config_invalid: {field} must be an array"
        ));
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                format!("cli_agent_config_invalid: {field}[{index}] must be a string")
            })
        })
        .collect()
}

fn run_cli_agent_with_adapter(
    adapter: &adapters::AdapterSpec,
    target_config: &serde_json::Value,
    task: &TaskSpec,
    workspace: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<CliAgentRun, String> {
    ensure_not_cancelled(is_cancelled)?;
    let Some(command) = &adapter.command else {
        return Err("cli_not_found: adapter has no command".into());
    };
    if !adapters::command_exists(command) {
        return Err(format!(
            "cli_not_found: install {} or configure an absolute command path",
            command
        ));
    }
    let vars = cli_agent_template_vars(task, workspace, target_config);
    let args: Vec<String> = adapter
        .args
        .iter()
        .map(|arg| adapters::render_template(arg, &vars))
        .collect();
    let working_dir = cli_agent_working_dir(adapter, workspace, &vars);
    let env = cli_agent_env(adapter, target_config, &vars)?;
    let rendered_command = rendered_command_line(command, &args);
    let command_metadata = capture_cli_agent_command_metadata(
        adapter,
        &rendered_command,
        command,
        &working_dir,
        is_cancelled,
    )?;
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut command = command_in(&working_dir, command, &arg_refs);
    for (key, value) in &env {
        command.env(key, value);
    }
    let capture = run_command_capture_checked(
        command,
        Duration::from_secs(task.timeout_seconds),
        is_cancelled,
    )?;
    Ok(CliAgentRun {
        capture,
        command_metadata,
        working_dir,
        env,
    })
}

fn cli_agent_template_vars(
    task: &TaskSpec,
    workspace: &Path,
    target_config: &serde_json::Value,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    if let Some(config) = target_config.as_object() {
        for (key, value) in config {
            if let Some(value) = json_template_value(value) {
                vars.insert(key.clone(), value);
            }
        }
    }
    vars.insert("prompt".to_string(), task.prompt.clone());
    vars.insert(
        "workspace".to_string(),
        workspace.to_string_lossy().to_string(),
    );
    vars.insert(
        "max_turns".to_string(),
        task.max_turns.unwrap_or(4).to_string(),
    );
    vars
}

fn json_template_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn cli_agent_working_dir(
    adapter: &adapters::AdapterSpec,
    workspace: &Path,
    vars: &HashMap<String, String>,
) -> PathBuf {
    let Some(template) = adapter.working_dir.as_deref() else {
        return workspace.to_path_buf();
    };
    let rendered = adapters::render_template(template, vars);
    let rendered = rendered.trim();
    if rendered.is_empty() {
        return workspace.to_path_buf();
    }
    let path = PathBuf::from(rendered);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}

fn cli_agent_env(
    adapter: &adapters::AdapterSpec,
    target_config: &serde_json::Value,
    vars: &HashMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut env = BTreeMap::new();
    merge_cli_agent_env(&mut env, &adapter.env, vars)?;
    if let Some(value) = target_config.get("env") {
        merge_cli_agent_env(&mut env, value, vars)?;
    }
    Ok(env)
}

fn merge_cli_agent_env(
    target: &mut BTreeMap<String, String>,
    value: &serde_json::Value,
    vars: &HashMap<String, String>,
) -> Result<(), String> {
    let Some(env) = value.as_object() else {
        if value.is_null() {
            return Ok(());
        }
        return Err("cli_agent_config_invalid: env must be an object".into());
    };
    for (key, value) in env {
        let Some(value) = json_template_value(value) else {
            return Err(format!(
                "cli_agent_config_invalid: env.{key} must be a string, number, or boolean"
            ));
        };
        target.insert(key.clone(), adapters::render_template(&value, vars));
    }
    Ok(())
}

fn rendered_command_line(command: &str, args: &[String]) -> Vec<String> {
    let mut rendered = Vec::with_capacity(args.len() + 1);
    rendered.push(command.to_string());
    rendered.extend(args.iter().cloned());
    rendered
}

fn cli_agent_execution_error(capture: &CommandCapture) -> Option<String> {
    if capture.timed_out {
        return Some("timeout: CLI agent exceeded task timeout".into());
    }
    if capture.code.unwrap_or(1) != 0 {
        return Some(format!(
            "cli_agent_failed: {}",
            capture
                .stderr
                .lines()
                .chain(capture.stdout.lines())
                .find(|line| !line.trim().is_empty())
                .unwrap_or("non-zero exit")
        ));
    }
    None
}

fn run_prompt_target(
    target: &store::TargetRecord,
    task: &TaskSpec,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ModelClientOutput, String> {
    ensure_not_cancelled(is_cancelled)?;
    let client = ModelClient::for_target(target)?;
    let request = ModelClientRequest::benchmark_prompt(task);
    client.complete(&request, is_cancelled)
}

fn run_target_warmup(
    target: &store::TargetRecord,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    ensure_not_cancelled(is_cancelled)?;
    if !matches!(
        target.kind.as_str(),
        "mock" | "direct_model" | "harnessed_model"
    ) {
        return Ok(());
    }
    run_prompt_target(target, &warmup_task(), is_cancelled)
        .map(|_| ())
        .map_err(|err| format!("warmup_failed: {}", err))
}

fn warmup_task() -> TaskSpec {
    TaskSpec {
        id: "benchforge-warmup".into(),
        name: "BenchForge Warmup".into(),
        task_type: "prompt".into(),
        version: Some("0.1.0".into()),
        language: None,
        fixture: None,
        prompt: "Reply with exactly: OK".into(),
        timeout_seconds: 60,
        max_turns: None,
        weight: 1.0,
        scoring: ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: Some("OK".into()),
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: false,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        },
        source_path: PathBuf::new(),
    }
}

fn mock_prompt_output(request: &ModelClientRequest<'_>) -> ModelClientOutput {
    let content = match request.task_id {
        "benchforge-warmup" => "OK".to_string(),
        "llm-instruction-following-001" => "benchforge".to_string(),
        "llm-json-validity-001" => {
            r#"{"task":"benchmark","valid":true,"items":["local","cloud"]}"#.to_string()
        }
        "llm-summarization-001" => {
            "BenchForge benchmarks local and cloud language models with reproducible runs."
                .to_string()
        }
        "llm-core-classification-001" => {
            r#"{"category":"billing","priority":"high","sentiment":"negative"}"#.to_string()
        }
        "llm-core-extraction-001" => {
            r#"{"invoice_id":"INV-2048","vendor":"Northstar Labs","due_date":"2026-08-15","total_usd":913.47}"#.to_string()
        }
        "llm-core-arithmetic-001" => r#"{"answer":47}"#.to_string(),
        "llm-core-tool-call-001" => {
            r#"{"tool":"create_calendar_event","arguments":{"title":"Benchmark review","date":"2026-07-10","time":"09:30","duration_minutes":30,"timezone":"UTC"}}"#.to_string()
        }
        "llm-core-boundary-001" => {
            r#"{"allowed":false,"reason":"private_credentials"}"#.to_string()
        }
        "llm-core-synthesis-001" => {
            "BenchForge compares local and cloud models by throughput, latency, cost, artifacts, and reproducibility.".to_string()
        }
        "llm-practical-routing-001" => {
            r#"{"team":"payments","severity":"p1","action":"rollback"}"#.to_string()
        }
        "llm-practical-json-repair-001" => {
            r#"{"customer":"Acme West","seats":42,"plan":"enterprise","renewal_date":"2026-09-30","status":"at risk"}"#.to_string()
        }
        "llm-practical-cost-math-001" => {
            r#"{"input_cost":0.01875,"output_cost":0.0252,"total_cost":0.04395}"#.to_string()
        }
        "llm-practical-safety-boundary-001" => {
            r#"{"allowed":false,"reason":"private_credentials","safe_alternative":"rotate_token"}"#.to_string()
        }
        "llm-practical-entity-resolution-001" => {
            r#"{"account_id":"acct_17","account_name":"Northstar Labs","confidence":"high"}"#.to_string()
        }
        "llm-practical-contradiction-001" => {
            r#"{"contradiction":true,"topic":"termination_for_convenience","needs_review":true}"#
                .to_string()
        }
        "llm-practical-tool-payload-001" => {
            r#"{"tool":"create_support_ticket","arguments":{"account_id":"acct_17","title":"Checkout outage","team":"payments","priority":"P1"}}"#.to_string()
        }
        "llm-practical-decision-memo-001" => {
            "- Choose Model B for support automation.\n- JSON reliability matters more than lower cost here.\n- Accept higher latency to reduce broken tool calls.".to_string()
        }
        "llm-practical-evidence-grounding-001" => {
            r#"{"decision":"defer","blocking_claim_ids":["claim_2","claim_3"],"confidence":"high"}"#
                .to_string()
        }
        "llm-practical-pii-redaction-001" => {
            r#"{"redacted_note":"Call Maya Chen at [REDACTED_EMAIL] or [REDACTED_PHONE]. Her SSN is [REDACTED_SSN]. Account acct_17 needs SSO.","pii_types":["email","phone","ssn"],"preserved_account_id":"acct_17"}"#.to_string()
        }
        "llm-practical-model-tradeoff-001" => {
            r#"{"selected_model_id":"local-qwen-14b","reasons":["latency below 1000 ms","JSON validity meets threshold","zero marginal cost"]}"#.to_string()
        }
        "llm-practical-strict-format-001" => {
            "Recommendation: Model B; Reason: valid JSON matters".to_string()
        }
        "llm-practical-privacy-aware-routing-001" => {
            r#"{"preprocess":"redact_pii","selected_model_id":"cloud-balanced","fallback_model_id":"local-qwen-14b","reason_codes":["pii_redaction","json_validity","cost"]}"#.to_string()
        }
        "llm-practical-budget-cap-model-mix-001" => {
            r#"{"simple_route":"local-qwen-14b","critical_route":"cloud-mini","monthly_cost_usd":50,"meets_budget":true,"reason_codes":["budget","pass_rate","cost"]}"#.to_string()
        }
        "llm-practical-regression-triage-001" => {
            r#"{"release_decision":"hold_rollout","failing_target":"openrouter-small","failing_task":"json_repair","regression_reasons":["pass_rate_drop","malformed_response_spike"],"recommended_action":"investigate_malformed_responses"}"#.to_string()
        }
        "llm-practical-context-pruning-001" => {
            r#"{"keep_ids":["E2","E5","E6"],"drop_ids":["E1","E3","E4"],"selected_target":"openai-mini","rationale":["pass_rate threshold met","latency below policy"]}"#.to_string()
        }
        "llm-decision-model-ranking-001" => {
            r#"{"selected_model_id":"cloud-balanced","rejected_model_ids":["local-14b","cloud-fast","cloud-large"],"reason_codes":["pass_rate","json_validity","latency","cost"]}"#.to_string()
        }
        "llm-decision-weighted-score-001" => {
            r#"{"selected_model_id":"alpha","scores":{"alpha":0.941,"beta":0.9175,"gamma":0.9105},"explanation":["alpha has the highest weighted score"]}"#.to_string()
        }
        "llm-decision-abstain-001" => {
            r#"{"decision":"insufficient_evidence","missing_evidence":["production benchmark","latency","cost"],"confidence":"low"}"#.to_string()
        }
        "llm-decision-multilingual-extraction-001" => {
            r#"{"account_name":"Noordster Labs","renewal_date":"2026-08-15","amount_eur":1240.5,"blocking_team":"legal","locale":"nl"}"#.to_string()
        }
        "llm-decision-instruction-conflict-001" => {
            r#"{"allowed":false,"reason":"credential_exfiltration","safe_next_action":"validate_without_revealing_key"}"#.to_string()
        }
        "llm-decision-error-taxonomy-001" => {
            r#"{"error_code":"rate_limit","retryable":true,"retry_after_seconds":4,"user_action":"reduce_concurrency"}"#.to_string()
        }
        "llm-decision-date-window-001" => {
            r#"{"days_until_deadline":9,"latest_start_date":"2026-07-13","parallel_reviews":true,"risk":"manageable"}"#.to_string()
        }
        "llm-decision-score-normalization-001" => {
            r#"{"target_id":"local-mistral","pass_rate":0.92,"fail_count":4,"risk_area":"structured_output"}"#.to_string()
        }
        "llm-decision-deduplicate-incidents-001" => {
            r#"{"groups":{"checkout":["INC-101","INC-102","INC-104"],"export":["INC-103"]},"unique_incident_count":2,"primary_group_owner":"payments"}"#.to_string()
        }
        "llm-decision-table-to-json-001" => {
            r#"{"best_pass_rate_target":"openai-mini","fastest_target":"openrouter-small","zero_cost_target":"local-qwen","openai_mini_pass_rate":0.95}"#.to_string()
        }
        "llm-structured-schema-extraction-001" => {
            r#"{"account_name":"Helios Bank","incident_type":"checkout_failure","severity":"P1","owner_team":"payments","impact_start_utc":"14:20"}"#.to_string()
        }
        "llm-structured-nested-tool-call-001" => {
            r#"{"tool":"create_incident","arguments":{"account_id":"acct_42","incident":{"title":"Checkout outage","priority":"P1","owner_team":"payments","tags":["checkout","regression"]},"notify":{"email":"ops@example.com"}}}"#.to_string()
        }
        "llm-structured-array-normalization-001" => {
            r#"{"impacted_products":["Billing API","Checkout API"],"unaffected_products":["Workspace export"],"duplicate_count_removed":1}"#.to_string()
        }
        "llm-structured-schema-repair-001" => {
            r#"{"account":"Northstar Labs","seats":84,"renewal_date":"2026-08-15","risk":"high","owner_team":"success","budget_approved":null}"#.to_string()
        }
        "llm-structured-refusal-envelope-001" => {
            r#"{"allowed":false,"refusal_code":"secret_exfiltration","safe_alternative":"store_redacted_key_status","redaction_required":true}"#.to_string()
        }
        "llm-structured-numeric-unit-conversion-001" => {
            r#"{"prompt_mtokens":0.128,"completion_mtokens":0.0325,"total_cost_usd":0.0516,"tokens_per_second":3062.9771}"#.to_string()
        }
        "llm-grounded-needle-citation-001" => {
            r#"{"answer":"The Helios Bank checkout incident started at 14:20 UTC after deploy 2026.07.07.","incident_start_utc":"14:20","deploy_id":"2026.07.07","evidence_ids":["S-03"]}"#.to_string()
        }
        "llm-grounded-distractor-filter-001" => {
            r#"{"impacted_services":["Checkout API"],"unaffected_services":["Workspace export"],"excluded_distractors":["Billing API","Search indexing"],"evidence_ids":["B","D","E"]}"#.to_string()
        }
        "llm-grounded-contradiction-resolution-001" => {
            r#"{"owner_team":"payments","root_cause":"retry_storm_after_deploy","selected_source_id":"postmortem","rejected_source_ids":["runbook-old","chat-rumor"],"conflict_resolution":"freshest_authoritative_source"}"#.to_string()
        }
        "llm-grounded-unsupported-claim-001" => {
            r#"{"supported":false,"verdict":"insufficient_evidence","missing_evidence":["production incident-rate data"],"safe_answer":"The claim is not supported by the supplied context.","evidence_ids":["C4"]}"#.to_string()
        }
        "llm-grounded-multi-document-synthesis-001" => {
            r#"{"summary":"Checkout failures affected paid workspaces and were mitigated by rollback.","impact":"paid_workspace_checkout_failures","likely_trigger":"checkout_retry_settings_deploy","mitigation":"rollback","evidence_ids":["log-1","deploy-7","support-2","rollback-3"]}"#.to_string()
        }
        "llm-grounded-noisy-table-grounding-001" => {
            r#"{"best_pass_rate_target":"openai-mini","fastest_final_target":"openrouter-small","ignored_rows":["R2"],"openai_mini_pass_rate":0.95,"evidence_ids":["R3","R4"]}"#.to_string()
        }
        "llm-reliability-ambiguous-requirements-001" => {
            r#"{"decision":"ask_for_constraints","selected_model_id":null,"missing_constraints":["minimum acceptable pass rate","latency target","budget cap"],"safe_next_action":"collect_requirements"}"#.to_string()
        }
        "llm-reliability-instruction-hierarchy-001" => {
            r#"{"action":"extract_ticket","ticket_id":"BF-219","priority":"P2","reason_code":"ignored_untrusted_instruction"}"#.to_string()
        }
        "llm-reliability-context-recall-001" => {
            r#"{"account":"Helios Bank","owner":"Priya Shah","deadline":"2026-08-14","cloud_cost_usd_per_1k_tickets":2.10,"evidence_ids":["A3","A6","A8"]}"#.to_string()
        }
        "llm-reliability-format-pressure-001" => {
            r#"{"approved":true,"route":"local","explanation":["direct personal identifiers require local routing"]}"#.to_string()
        }
        "llm-reliability-multi-step-consistency-001" => {
            r#"{"eligible_model_ids":["cloud-mini"],"rejected_model_ids":["local-qwen","cloud-large"],"selected_model_id":"cloud-mini","pass_rates":{"local-qwen":0.875,"cloud-mini":0.95,"cloud-large":0.975},"reason":["cloud-mini is the lowest total_cost_usd eligible model"]}"#.to_string()
        }
        "llm-reliability-correction-discipline-001" => {
            r#"{"draft_is_correct":false,"corrected_selected_model_id":"boreal-cloud","correction_reasons":["atlas-local pass_rate below threshold","boreal-cloud lowest eligible cost"],"atlas_pass_rate":0.875}"#.to_string()
        }
        "llm-reliability-sample-size-caution-001" => {
            r#"{"decision":"collect_more_evidence","selected_model_id":null,"evidence_risks":["coverage_gap","low_repetitions","single_sample_pass_rates"],"minimum_next_run":"Run every visible target on the same task set with at least 3 repetitions per task."}"#.to_string()
        }
        "llm-reliability-confidence-interval-overlap-001" => {
            r#"{"decision":"inconclusive","selected_model_id":null,"close_contender_ids":["local-14b","cloud-mini"],"uncertainty_reasons":["confidence_intervals_overlap","point_estimate_not_enough"],"minimum_next_run":"Rerun the same pack with more repetitions per task before selecting a winner."}"#.to_string()
        }
        "llm-reliability-privacy-preserving-eval-001" => {
            r#"{"privacy_action":"redact_before_cloud_or_local_only","raw_cloud_allowed":false,"allowed_raw_targets":["local-llama"],"blocked_raw_targets":["cloud-mini","cloud-large"],"sanitized_cloud_allowed":true,"next_run_pack":"llm-reliability","rationale_codes":["pii_present","local_raw_allowed","cloud_requires_redaction"]}"#.to_string()
        }
        "llm-reliability-served-model-identity-001" => {
            r#"{"decision":"block_model_selection","recommendation_allowed":false,"blocked_target_ids":["local-qwen","cloud-router"],"identity_risks":["configured_fallback_identity","served_model_mismatch"],"next_action":"revalidate_targets_and_rerun_same_pack","report_note":"Require a provider-confirmed served model id for every compared target before making a model-selection recommendation."}"#.to_string()
        }
        "llm-reliability-latency-cost-slo-001" => {
            r#"{"eligible_model_ids":["cloud-mini"],"rejected_model_ids":["local-mlx","local-qwen","cloud-large"],"selected_model_id":"cloud-mini","rejection_reasons":{"local-mlx":["pass_rate below threshold"],"local-qwen":["p95_latency_ms above limit"],"cloud-large":["p95_latency_ms above limit","cost_usd_per_1k_tickets above cap"]},"reason":"cloud-mini is the lowest cost eligible model."}"#.to_string()
        }
        "llm-reliability-rate-limit-retry-001" => {
            r#"{"root_cause":"rate_limited","retryable":true,"recommended_action":"reduce_concurrency_and_retry","config_changes":["lower_concurrency","honor_retry_after","increase_provider_retries"],"do_not_conclude":"model_quality_regression"}"#.to_string()
        }
        _ => "OK".to_string(),
    };
    let mut metrics = serde_json::Map::new();
    metrics.insert("mock".into(), serde_json::json!(true));
    ModelClientOutput {
        content,
        raw_response: None,
        metrics,
    }
}

fn call_openai_prompt(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
    request: &ModelClientRequest<'_>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ModelClientOutput, String> {
    ensure_not_cancelled(is_cancelled)?;
    let base_url = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
        .ok_or_else(|| "provider_skipped: base_url is missing".to_string())?;
    let model = config
        .get("model")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "provider_skipped: model is missing".to_string())?;
    let secret_env = config
        .get("api_key_env")
        .and_then(|value| value.as_str())
        .or_else(|| {
            adapter
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())
        });
    let api_key = config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        .and_then(crate::secrets::read_cloud_api_key)
        .or_else(|| secret_env.and_then(|name| std::env::var(name).ok()))
        .unwrap_or_default();
    let mut payload = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": request.system_prompt.as_ref()},
            {"role": "user", "content": request.user_prompt.as_ref()}
        ],
        "temperature": generation_temperature(config),
        "top_p": generation_top_p(config),
        "max_tokens": generation_max_tokens(config, request.default_max_tokens)
    });
    insert_optional_seed(&mut payload, config);
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let headers = vec![
        ("Content-Type", "application/json".to_string()),
        ("Authorization", format!("Bearer {}", api_key)),
    ];
    if streaming_enabled(adapter, config) {
        match provider_stream_with_retry(config, is_cancelled, || {
            let stream_payload = openai_chat_stream_payload(&payload, adapter, config);
            post_json_stream_with_curl(
                &url,
                &stream_payload,
                &headers,
                request_timeout_seconds(config),
                StreamFormat::OpenAiChat,
                is_cancelled,
            )
        }) {
            Ok(mut provider) => {
                maybe_confirm_local_openai_runtime_model(
                    &mut provider.metrics,
                    base_url,
                    model,
                    config,
                    &headers,
                    is_cancelled,
                );
                return Ok(stream_prompt_output(provider));
            }
            Err(err) if should_fallback_streaming_error(&err) => {}
            Err(err) => return Err(err),
        }
    }
    let provider = provider_json_with_retry(config, is_cancelled, || {
        post_json_with_curl(
            &url,
            &payload,
            &headers,
            request_timeout_seconds(config),
            is_cancelled,
        )
    })?;
    let content = provider
        .json
        .pointer("/choices/0/message/content")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            "malformed_response: provider response did not include choices[0].message.content"
                .to_string()
        })?;
    let mut metrics = openai_response_metrics(&provider.json);
    metrics.insert(
        "provider_attempts".into(),
        serde_json::json!(provider.attempts),
    );
    insert_provider_transport_metrics(&mut metrics, &provider);
    maybe_confirm_local_openai_runtime_model(
        &mut metrics,
        base_url,
        model,
        config,
        &headers,
        is_cancelled,
    );
    Ok(ModelClientOutput {
        content,
        raw_response: Some(provider.raw),
        metrics,
    })
}

fn call_openai_responses_prompt(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
    request: &ModelClientRequest<'_>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ModelClientOutput, String> {
    ensure_not_cancelled(is_cancelled)?;
    let base_url = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
        .ok_or_else(|| "provider_skipped: OpenAI base_url is missing".to_string())?;
    let model = config
        .get("model")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "provider_skipped: OpenAI model is missing".to_string())?;
    let api_key = openai_api_key(adapter, config)?;
    let payload = openai_responses_payload(
        model,
        request.system_prompt.as_ref(),
        request.user_prompt.as_ref(),
        config,
        request.default_max_tokens,
    );
    let url = openai_responses_url(base_url);
    let headers = vec![
        ("Content-Type", "application/json".to_string()),
        ("Authorization", format!("Bearer {}", api_key)),
    ];
    if streaming_enabled(adapter, config) {
        match provider_stream_with_retry(config, is_cancelled, || {
            let stream_payload = streaming_payload(&payload);
            post_json_stream_with_curl(
                &url,
                &stream_payload,
                &headers,
                request_timeout_seconds(config),
                StreamFormat::OpenAiResponses,
                is_cancelled,
            )
        }) {
            Ok(provider) => return Ok(stream_prompt_output(provider)),
            Err(err) if should_fallback_streaming_error(&err) => {}
            Err(err) => return Err(err),
        }
    }
    let provider = provider_json_with_retry(config, is_cancelled, || {
        post_json_with_curl(
            &url,
            &payload,
            &headers,
            request_timeout_seconds(config),
            is_cancelled,
        )
    })?;
    let content = openai_responses_text(&provider.json).ok_or_else(|| {
        "malformed_response: OpenAI Responses payload did not include output text".to_string()
    })?;
    let mut metrics = openai_responses_metrics(&provider.json);
    metrics.insert(
        "provider_attempts".into(),
        serde_json::json!(provider.attempts),
    );
    insert_provider_transport_metrics(&mut metrics, &provider);
    Ok(ModelClientOutput {
        content,
        raw_response: Some(provider.raw),
        metrics,
    })
}

fn call_anthropic_prompt(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
    request: &ModelClientRequest<'_>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ModelClientOutput, String> {
    ensure_not_cancelled(is_cancelled)?;
    let base_url = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
        .unwrap_or("https://api.anthropic.com");
    let model = config
        .get("model")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "provider_skipped: model is missing".to_string())?;
    let secret_env = config
        .get("api_key_env")
        .and_then(|value| value.as_str())
        .unwrap_or("ANTHROPIC_API_KEY");
    let api_key = config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        .and_then(crate::secrets::read_cloud_api_key)
        .or_else(|| std::env::var(secret_env).ok())
        .ok_or_else(|| {
            format!(
                "provider_skipped: no Keychain key configured and {} is not set",
                secret_env
            )
        })?;
    let payload = serde_json::json!({
        "model": model,
        "max_tokens": generation_max_tokens(config, request.default_max_tokens),
        "temperature": generation_temperature(config),
        "top_p": generation_top_p(config),
        "system": request.system_prompt.as_ref(),
        "messages": [{"role": "user", "content": request.user_prompt.as_ref()}]
    });
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let headers = vec![
        ("Content-Type", "application/json".to_string()),
        ("x-api-key", api_key.clone()),
        ("anthropic-version", "2023-06-01".to_string()),
    ];
    if streaming_enabled(adapter, config) {
        match provider_stream_with_retry(config, is_cancelled, || {
            let stream_payload = streaming_payload(&payload);
            post_json_stream_with_curl(
                &url,
                &stream_payload,
                &headers,
                request_timeout_seconds(config),
                StreamFormat::AnthropicMessages,
                is_cancelled,
            )
        }) {
            Ok(provider) => return Ok(stream_prompt_output(provider)),
            Err(err) if should_fallback_streaming_error(&err) => {}
            Err(err) => return Err(err),
        }
    }
    let provider = provider_json_with_retry(config, is_cancelled, || {
        post_json_with_curl(
            &url,
            &payload,
            &headers,
            request_timeout_seconds(config),
            is_cancelled,
        )
    })?;
    let content = provider
        .json
        .pointer("/content/0/text")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            "malformed_response: provider response did not include content[0].text".to_string()
        })?;
    let mut metrics = anthropic_response_metrics(&provider.json);
    metrics.insert(
        "provider_attempts".into(),
        serde_json::json!(provider.attempts),
    );
    insert_provider_transport_metrics(&mut metrics, &provider);
    Ok(ModelClientOutput {
        content,
        raw_response: Some(provider.raw),
        metrics,
    })
}

fn call_azure_openai_prompt(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
    request: &ModelClientRequest<'_>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ModelClientOutput, String> {
    ensure_not_cancelled(is_cancelled)?;
    let base_url = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .or(adapter.default_base_url.as_deref())
        .ok_or_else(|| "provider_skipped: Azure OpenAI base_url is missing".to_string())?;
    let model = config
        .get("model")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "provider_skipped: Azure OpenAI deployment/model is missing".to_string())?;
    let api_key = azure_openai_api_key(adapter, config)?;
    let mut payload = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": request.system_prompt.as_ref()},
            {"role": "user", "content": request.user_prompt.as_ref()}
        ],
        "temperature": generation_temperature(config),
        "top_p": generation_top_p(config),
        "max_tokens": generation_max_tokens(config, request.default_max_tokens)
    });
    remove_azure_legacy_model_field(base_url, &mut payload);
    insert_optional_seed(&mut payload, config);
    let url = azure_openai_chat_url(base_url, model, config);
    let headers = vec![
        ("Content-Type", "application/json".to_string()),
        ("api-key", api_key.clone()),
    ];
    if streaming_enabled(adapter, config) {
        match provider_stream_with_retry(config, is_cancelled, || {
            let stream_payload = openai_chat_stream_payload(&payload, adapter, config);
            post_json_stream_with_curl(
                &url,
                &stream_payload,
                &headers,
                request_timeout_seconds(config),
                StreamFormat::OpenAiChat,
                is_cancelled,
            )
        }) {
            Ok(provider) => return Ok(stream_prompt_output(provider)),
            Err(err) if should_fallback_streaming_error(&err) => {}
            Err(err) => return Err(err),
        }
    }
    let provider = provider_json_with_retry(config, is_cancelled, || {
        post_json_with_curl(
            &url,
            &payload,
            &headers,
            request_timeout_seconds(config),
            is_cancelled,
        )
    })?;
    let content = provider
        .json
        .pointer("/choices/0/message/content")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            "malformed_response: provider response did not include choices[0].message.content"
                .to_string()
        })?;
    let mut metrics = openai_response_metrics(&provider.json);
    metrics.insert(
        "provider_attempts".into(),
        serde_json::json!(provider.attempts),
    );
    insert_provider_transport_metrics(&mut metrics, &provider);
    Ok(ModelClientOutput {
        content,
        raw_response: Some(provider.raw),
        metrics,
    })
}

fn run_direct_model_target(
    target: &store::TargetRecord,
    request: &ModelClientRequest<'_>,
    workspace: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ModelClientOutput, ModelExecutionError> {
    ensure_not_cancelled(is_cancelled).map_err(ModelExecutionError::without_output)?;
    let config: serde_json::Value =
        serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(mock_output) = config.get("mock_output").and_then(|value| value.as_str()) {
        let metrics = config
            .get("mock_metrics")
            .and_then(|value| value.as_object())
            .cloned()
            .unwrap_or_else(|| {
                let mut metrics = serde_json::Map::new();
                metrics.insert("mock".into(), serde_json::json!(true));
                metrics
            });
        let output = ModelClientOutput {
            content: mock_output.to_string(),
            raw_response: None,
            metrics,
        };
        if let Err(err) = apply_model_output(workspace, mock_output, is_cancelled) {
            return Err(ModelExecutionError::with_output(err, output));
        }
        return Ok(output);
    }
    let client = ModelClient::for_target(target).map_err(ModelExecutionError::without_output)?;
    let output = client
        .complete(request, is_cancelled)
        .map_err(ModelExecutionError::without_output)?;
    if let Err(err) = ensure_not_cancelled(is_cancelled) {
        return Err(ModelExecutionError::with_output(err, output));
    }
    if let Err(err) = apply_model_output(workspace, &output.content, is_cancelled) {
        return Err(ModelExecutionError::with_output(err, output));
    }
    Ok(output)
}

fn openai_response_metrics(json: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    let mut metrics = serde_json::Map::new();
    if let Some(model) = json.get("model").and_then(|value| value.as_str()) {
        metrics.insert("provider_model".into(), serde_json::json!(model));
    }
    if let Some(reason) = json
        .pointer("/choices/0/finish_reason")
        .and_then(|value| value.as_str())
    {
        metrics.insert("finish_reason".into(), serde_json::json!(reason));
    }
    if let Some(usage) = json.get("usage") {
        insert_openai_usage_metrics(&mut metrics, usage);
    }
    metrics
}

fn maybe_confirm_local_openai_runtime_model(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    base_url: &str,
    model: &str,
    config: &serde_json::Value,
    headers: &[(&str, String)],
    is_cancelled: &dyn Fn() -> bool,
) {
    let has_provider_model = metrics
        .get("provider_model")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty());
    if has_provider_model {
        return;
    }
    let configured_model = model.trim();
    if configured_model.is_empty() {
        return;
    }
    let lower_base_url = base_url.to_ascii_lowercase();
    if !lower_base_url.starts_with("http") || !target_base_url_is_local(&lower_base_url) {
        return;
    }
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let timeout_seconds = request_timeout_seconds(config).clamp(1, 5);
    let Ok(response) = get_json_with_curl(&url, headers, timeout_seconds, is_cancelled) else {
        return;
    };
    if provider_http_status_error_with_retry(
        response.status,
        &response.body,
        response.retry_after_ms,
    )
    .is_some()
    {
        return;
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&response.body) else {
        return;
    };
    if openai_runtime_model_ids(&json)
        .iter()
        .any(|id| id == configured_model)
    {
        metrics.insert("provider_model".into(), serde_json::json!(configured_model));
        metrics.insert(
            "provider_model_source".into(),
            serde_json::json!("runtime_models"),
        );
    }
}

fn openai_runtime_model_ids(json: &serde_json::Value) -> Vec<String> {
    let mut ids = Vec::new();
    for list in [
        json.as_array(),
        json.get("data").and_then(|value| value.as_array()),
        json.get("models").and_then(|value| value.as_array()),
    ]
    .into_iter()
    .flatten()
    {
        for item in list {
            let model_id = item
                .as_str()
                .or_else(|| item.get("id").and_then(|value| value.as_str()))
                .or_else(|| item.get("name").and_then(|value| value.as_str()))
                .or_else(|| item.get("model").and_then(|value| value.as_str()))
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(model_id) = model_id {
                ids.push(model_id.to_string());
                if ids.len() >= 100 {
                    return ids;
                }
            }
        }
    }
    ids
}

fn insert_provider_transport_metrics(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    provider: &ProviderJsonResponse,
) {
    insert_provider_timing_metrics(
        metrics,
        provider.http_status,
        provider.retry_after_ms,
        provider.retry_delay_ms,
        provider.time_to_first_byte_ms,
        provider.time_to_first_token_ms,
        provider.request_total_ms,
    );
}

fn insert_provider_stream_transport_metrics(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    provider: &ProviderStreamResponse,
) {
    insert_provider_timing_metrics(
        metrics,
        provider.http_status,
        provider.retry_after_ms,
        provider.retry_delay_ms,
        provider.time_to_first_byte_ms,
        provider.time_to_first_token_ms,
        provider.request_total_ms,
    );
}

fn insert_provider_timing_metrics(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    http_status: Option<u16>,
    retry_after_ms: Option<u64>,
    retry_delay_ms: Option<u64>,
    time_to_first_byte_ms: Option<f64>,
    time_to_first_token_ms: Option<f64>,
    request_total_ms: Option<f64>,
) {
    if let Some(status) = http_status {
        metrics.insert("http_status".into(), serde_json::json!(status));
    }
    if let Some(value) = retry_after_ms {
        metrics.insert("provider_retry_after_ms".into(), serde_json::json!(value));
    }
    if let Some(value) = retry_delay_ms {
        metrics.insert("provider_retry_delay_ms".into(), serde_json::json!(value));
    }
    if let Some(value) = time_to_first_byte_ms {
        metrics.insert(
            "provider_time_to_first_byte_ms".into(),
            serde_json::json!(value),
        );
    }
    if let Some(value) = time_to_first_token_ms {
        metrics.insert(
            "provider_time_to_first_token_ms".into(),
            serde_json::json!(value),
        );
    }
    if let Some(value) = request_total_ms {
        metrics.insert("provider_request_total_ms".into(), serde_json::json!(value));
    }
}

fn openai_api_key(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Result<String, String> {
    let secret_env = config
        .get("api_key_env")
        .and_then(|value| value.as_str())
        .or_else(|| {
            adapter
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())
        })
        .unwrap_or("OPENAI_API_KEY");
    config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        .and_then(crate::secrets::read_cloud_api_key)
        .or_else(|| std::env::var(secret_env).ok())
        .ok_or_else(|| {
            format!(
                "provider_skipped: no Keychain key configured and {} is not set",
                secret_env
            )
        })
}

fn openai_responses_url(base_url: &str) -> String {
    format!("{}/responses", base_url.trim_end_matches('/'))
}

fn openai_responses_payload(
    model: &str,
    instructions: &str,
    user_content: &str,
    config: &serde_json::Value,
    default_max_tokens: u64,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "instructions": instructions,
        "input": [
            {
                "role": "user",
                "content": user_content
            }
        ],
        "temperature": generation_temperature(config),
        "top_p": generation_top_p(config),
        "max_output_tokens": generation_max_tokens(config, default_max_tokens),
        "store": false
    })
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

fn openai_responses_metrics(
    json: &serde_json::Value,
) -> serde_json::Map<String, serde_json::Value> {
    let mut metrics = serde_json::Map::new();
    if let Some(model) = json.get("model").and_then(|value| value.as_str()) {
        metrics.insert("provider_model".into(), serde_json::json!(model));
    }
    if let Some(status) = json.get("status").and_then(|value| value.as_str()) {
        metrics.insert("response_status".into(), serde_json::json!(status));
        metrics.insert("finish_reason".into(), serde_json::json!(status));
    }
    if let Some(usage) = json.get("usage") {
        insert_openai_usage_metrics(&mut metrics, usage);
    }
    metrics
}

fn anthropic_response_metrics(
    json: &serde_json::Value,
) -> serde_json::Map<String, serde_json::Value> {
    let mut metrics = serde_json::Map::new();
    if let Some(model) = json.get("model").and_then(|value| value.as_str()) {
        metrics.insert("provider_model".into(), serde_json::json!(model));
    }
    if let Some(reason) = json.get("stop_reason").and_then(|value| value.as_str()) {
        metrics.insert("finish_reason".into(), serde_json::json!(reason));
    }
    if let Some(usage) = json.get("usage") {
        insert_anthropic_usage_metrics(&mut metrics, usage);
    }
    metrics
}

fn insert_provider_metrics(
    conn: &Connection,
    run_id: &str,
    metrics: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    for (name, value) in metrics {
        if let Some(number) = value.as_f64() {
            let unit = if name.ends_with("_tokens") {
                Some("tokens")
            } else {
                None
            };
            store::insert_metric(conn, run_id, name, Some(number), unit, "provider")
                .map_err(|err| err.to_string())?;
        } else if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
            store::insert_metric_text(conn, run_id, name, text, "provider")
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn ensure_provider_model_metric(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    target_config: &serde_json::Value,
) {
    let has_provider_model = metrics
        .get("provider_model")
        .and_then(|value| value.as_str())
        .is_some_and(|text| !text.trim().is_empty());
    if has_provider_model {
        metrics
            .entry("provider_model_source")
            .or_insert_with(|| serde_json::json!("provider"));
        return;
    }
    if let Some(model) = target_config
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        metrics.insert("provider_model".into(), serde_json::json!(model));
        metrics.insert(
            "provider_model_source".into(),
            serde_json::json!("target_config"),
        );
    }
}

fn estimate_cost_usd(
    kind: &str,
    adapter_id: &str,
    config: &serde_json::Value,
    metrics: &serde_json::Map<String, serde_json::Value>,
) -> Option<f64> {
    let input_price = price_per_million(config, "input_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "input_usd_per_million_tokens"));
    let output_price = price_per_million(config, "output_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "output_usd_per_million_tokens"));
    let cache_read_price = price_per_million(config, "cache_read_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "cached_input_price_usd_per_million_tokens"));
    let cache_write_price = price_per_million(config, "cache_write_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "cache_creation_price_usd_per_million_tokens"));
    if input_price.is_none()
        && output_price.is_none()
        && targeting::target_is_known_zero_cost_when_unpriced(kind, adapter_id, config)
    {
        return Some(0.0);
    }
    let prompt_tokens = metrics
        .get("prompt_tokens")
        .and_then(|value| value.as_f64())?;
    let completion_tokens = metrics
        .get("completion_tokens")
        .and_then(|value| value.as_f64())?;
    let input_price = input_price?;
    let output_price = output_price?;
    let cache_read_tokens = metric_number(metrics, &["cache_read_tokens", "cached_tokens"])
        .unwrap_or(0.0)
        .max(0.0);
    let cache_write_tokens = metric_number(metrics, &["cache_write_tokens"])
        .unwrap_or(0.0)
        .max(0.0);
    let prompt_tokens_include_cache = prompt_tokens_include_cache_tokens(adapter_id, config);
    let input_cost = if prompt_tokens_include_cache {
        let replaced_cache_tokens = cache_read_price.map(|_| cache_read_tokens).unwrap_or(0.0)
            + cache_write_price.map(|_| cache_write_tokens).unwrap_or(0.0);
        (prompt_tokens - replaced_cache_tokens).max(0.0) * input_price
            + cache_read_price
                .map(|price| cache_read_tokens * price)
                .unwrap_or(0.0)
            + cache_write_price
                .map(|price| cache_write_tokens * price)
                .unwrap_or(0.0)
    } else {
        prompt_tokens * input_price
            + cache_read_tokens * cache_read_price.unwrap_or(input_price)
            + cache_write_tokens * cache_write_price.unwrap_or(input_price)
    };
    Some((input_cost + completion_tokens * output_price) / 1_000_000.0)
}

fn metric_number(
    metrics: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<f64> {
    keys.iter()
        .find_map(|key| metrics.get(*key).and_then(|value| value.as_f64()))
}

fn prompt_tokens_include_cache_tokens(adapter_id: &str, config: &serde_json::Value) -> bool {
    for key in [
        "prompt_tokens_include_cache_tokens",
        "input_tokens_include_cache_tokens",
    ] {
        if let Some(value) = config.get(key).and_then(|value| value.as_bool()) {
            return value;
        }
    }
    adapter_id != "anthropic"
}

const CACHE_READ_PRICED_AS_INPUT: &str = "cache_read_tokens_priced_as_input";
const CACHE_WRITE_PRICED_AS_INPUT: &str = "cache_write_tokens_priced_as_input";

fn cache_pricing_fallback_assumptions(
    config: &serde_json::Value,
    metrics: &serde_json::Map<String, serde_json::Value>,
) -> Vec<&'static str> {
    let input_price = price_per_million(config, "input_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "input_usd_per_million_tokens"));
    let output_price = price_per_million(config, "output_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "output_usd_per_million_tokens"));
    if input_price.is_none()
        || output_price.is_none()
        || metric_number(metrics, &["prompt_tokens"]).is_none()
        || metric_number(metrics, &["completion_tokens"]).is_none()
    {
        return Vec::new();
    }

    let cache_read_price = price_per_million(config, "cache_read_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "cached_input_price_usd_per_million_tokens"));
    let cache_write_price = price_per_million(config, "cache_write_price_usd_per_million_tokens")
        .or_else(|| price_per_million(config, "cache_creation_price_usd_per_million_tokens"));
    let cache_read_tokens = metric_number(metrics, &["cache_read_tokens", "cached_tokens"])
        .unwrap_or(0.0)
        .max(0.0);
    let cache_write_tokens = metric_number(metrics, &["cache_write_tokens"])
        .unwrap_or(0.0)
        .max(0.0);

    let mut assumptions = Vec::new();
    if cache_read_tokens > 0.0 && cache_read_price.is_none() {
        assumptions.push(CACHE_READ_PRICED_AS_INPUT);
    }
    if cache_write_tokens > 0.0 && cache_write_price.is_none() {
        assumptions.push(CACHE_WRITE_PRICED_AS_INPUT);
    }
    assumptions
}

fn insert_pricing_assumption_metrics(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    assumptions: &[&str],
) {
    if assumptions.is_empty() {
        return;
    }
    if assumptions.contains(&CACHE_READ_PRICED_AS_INPUT) {
        metrics.insert(
            "cache_read_priced_with_input_price".into(),
            serde_json::json!(1.0),
        );
    }
    if assumptions.contains(&CACHE_WRITE_PRICED_AS_INPUT) {
        metrics.insert(
            "cache_write_priced_with_input_price".into(),
            serde_json::json!(1.0),
        );
    }
    metrics.insert(
        "pricing_assumption".into(),
        serde_json::json!(assumptions.join(";")),
    );
}

fn insert_pricing_assumption_store_metrics(
    conn: &Connection,
    run_id: &str,
    assumptions: &[&str],
) -> Result<(), String> {
    if assumptions.is_empty() {
        return Ok(());
    }
    if assumptions.contains(&CACHE_READ_PRICED_AS_INPUT) {
        store::insert_metric(
            conn,
            run_id,
            "cache_read_priced_with_input_price",
            Some(1.0),
            None,
            "pricing",
        )
        .map_err(|err| err.to_string())?;
    }
    if assumptions.contains(&CACHE_WRITE_PRICED_AS_INPUT) {
        store::insert_metric(
            conn,
            run_id,
            "cache_write_priced_with_input_price",
            Some(1.0),
            None,
            "pricing",
        )
        .map_err(|err| err.to_string())?;
    }
    store::insert_metric_text(
        conn,
        run_id,
        "pricing_assumption",
        &assumptions.join(";"),
        "pricing",
    )
    .map_err(|err| err.to_string())
}

fn pricing_assumption_warnings(assumptions: &[&str]) -> Vec<String> {
    assumptions
        .iter()
        .map(|assumption| match *assumption {
            CACHE_READ_PRICED_AS_INPUT => {
                "pricing_assumption: cache read tokens were priced at the normal input-token price because cache read pricing is not configured"
            }
            CACHE_WRITE_PRICED_AS_INPUT => {
                "pricing_assumption: cache write tokens were priced at the normal input-token price because cache write pricing is not configured"
            }
            other => other,
        })
        .map(str::to_string)
        .collect()
}

fn estimate_output_tokens_per_second(
    metrics: &serde_json::Map<String, serde_json::Value>,
    wall_time_ms: u64,
) -> Option<f64> {
    if wall_time_ms == 0 {
        return None;
    }
    let completion_tokens = metrics
        .get("completion_tokens")
        .and_then(|value| value.as_f64())?;
    if completion_tokens < 0.0 {
        return None;
    }
    Some(completion_tokens / (wall_time_ms as f64 / 1_000.0))
}

fn insert_v1_metric_aliases(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    status: &str,
    score: Option<f64>,
) {
    metrics.insert("pass_fail".into(), serde_json::json!(status == "passed"));
    if let Some(value) = score {
        metrics
            .entry("score_numeric")
            .or_insert_with(|| serde_json::json!(value));
    }
    copy_metric_alias(metrics, "prompt_tokens", "input_tokens");
    copy_metric_alias(metrics, "completion_tokens", "output_tokens");
    copy_metric_alias(metrics, "cost_usd", "estimated_cost_usd");
    copy_metric_alias(metrics, "provider_time_to_first_token_ms", "ttft_ms");
    copy_metric_alias(metrics, "output_tokens_per_second", "decode_tokens_per_sec");
    insert_required_v1_metric_nulls(metrics);
}

fn copy_metric_alias(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    from: &str,
    to: &str,
) {
    if metrics.contains_key(to) {
        return;
    }
    if let Some(value) = metrics.get(from).cloned() {
        metrics.insert(to.into(), value);
    }
}

fn normalize_result_event_metrics(event: &mut serde_json::Value, status: &str, score: Option<f64>) {
    if !event.get("metrics").is_some_and(|value| value.is_object()) {
        event["metrics"] = serde_json::json!({});
    }
    if let Some(metrics) = event
        .get_mut("metrics")
        .and_then(|value| value.as_object_mut())
    {
        insert_v1_metric_aliases(metrics, status, score);
    }
}

fn worker_import_reproducibility(event: &serde_json::Value) -> Option<serde_json::Value> {
    if event.get("imported").and_then(|value| value.as_bool()) != Some(true) {
        return None;
    }
    Some(serde_json::json!({
        "path": event.get("import_path").cloned().unwrap_or(serde_json::Value::Null),
        "format": event.get("import_format").cloned().unwrap_or(serde_json::Value::Null),
        "formats": event.get("import_formats").cloned().unwrap_or(serde_json::Value::Null),
        "source": event.get("import_source").cloned().unwrap_or(serde_json::Value::Null),
        "read_files": event.get("import_read_files").cloned().unwrap_or(serde_json::Value::Null),
        "hash_algorithm": event.get("import_hash_algorithm").cloned().unwrap_or(serde_json::Value::Null),
        "file_details": event.get("import_file_details").cloned().unwrap_or(serde_json::Value::Null),
        "file_count": event.get("import_files").cloned().unwrap_or(serde_json::Value::Null),
        "total_file_count": event.get("import_total_files").cloned().unwrap_or(serde_json::Value::Null),
        "omitted_file_count": event.get("import_omitted_files").cloned().unwrap_or(serde_json::Value::Null),
        "truncated": event.get("import_truncated").cloned().unwrap_or(serde_json::Value::Null),
        "truncated_bytes": event.pointer("/metrics/import_truncated_bytes").cloned().unwrap_or(serde_json::Value::Null),
        "summary_source": event
            .pointer("/tests/summary_source")
            .cloned()
            .or_else(|| event.pointer("/metrics/summary_source").cloned())
            .unwrap_or(serde_json::Value::Null),
    }))
}

fn ensure_result_event_metric(event: &mut serde_json::Value, key: &str, value: serde_json::Value) {
    if !event.get("metrics").is_some_and(|value| value.is_object()) {
        event["metrics"] = serde_json::json!({});
    }
    if event
        .get("metrics")
        .and_then(|metrics| metrics.get(key))
        .is_none()
    {
        event["metrics"][key] = value;
    }
}

fn ensure_result_event_command_capture_metrics(
    event: &mut serde_json::Value,
    capture: &CommandCapture,
) {
    ensure_result_event_metric(
        event,
        "wall_time_ms",
        serde_json::json!(capture.wall_time_ms),
    );
    ensure_result_event_metric(event, "exit_code", serde_json::json!(capture.code));
    ensure_result_event_metric(
        event,
        "stdout_bytes",
        serde_json::json!(capture.stdout.len()),
    );
    ensure_result_event_metric(
        event,
        "stderr_bytes",
        serde_json::json!(capture.stderr.len()),
    );
    if let Some(value) = capture.peak_rss_mb {
        ensure_result_event_metric(event, "peak_rss_mb", serde_json::json!(value));
    }
}

fn insert_required_v1_metric_nulls(metrics: &mut serde_json::Map<String, serde_json::Value>) {
    for key in REQUIRED_V1_METRIC_KEYS {
        metrics
            .entry((*key).to_string())
            .or_insert(serde_json::Value::Null);
    }
}

const REQUIRED_V1_METRIC_KEYS: &[&str] = &[
    "pass_fail",
    "score_numeric",
    "wall_time_ms",
    "setup_time_ms",
    "target_time_ms",
    "evaluation_time_ms",
    "exit_code",
    "stdout_bytes",
    "stderr_bytes",
    "files_changed",
    "lines_added",
    "lines_deleted",
    "commands_observed_count",
    "dangerous_command_hits",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "estimated_cost_usd",
    "ttft_ms",
    "decode_tokens_per_sec",
    "peak_rss_mb",
];

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

fn azure_openai_api_key(
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> Result<String, String> {
    let secret_env = config
        .get("api_key_env")
        .and_then(|value| value.as_str())
        .or_else(|| {
            adapter
                .validation
                .get("secret_env")
                .and_then(|value| value.as_str())
        })
        .unwrap_or("AZURE_OPENAI_API_KEY");
    config
        .get("api_key_keychain")
        .and_then(|value| value.as_str())
        .and_then(crate::secrets::read_cloud_api_key)
        .or_else(|| std::env::var(secret_env).ok())
        .ok_or_else(|| {
            format!(
                "provider_skipped: no Keychain key configured and {} is not set",
                secret_env
            )
        })
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

fn target_reproducibility(
    target: &store::TargetRecord,
    config: &serde_json::Value,
) -> serde_json::Value {
    let mut target_json = serde_json::json!({
        "id": target.id,
        "adapter_id": target.adapter_id,
        "kind": target.kind,
        "config": redact_target_config(config),
    });
    if let Some(local_model) = local_model_fingerprint(config) {
        if let Some(object) = target_json.as_object_mut() {
            object.insert("local_model".into(), local_model);
        }
    }
    target_json
}

fn redact_target_config(config: &serde_json::Value) -> serde_json::Value {
    match config {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if is_secret_config_key(key) {
                    out.insert(key.clone(), serde_json::json!("<redacted>"));
                } else {
                    out.insert(key.clone(), redact_target_config(value));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(redact_target_config).collect())
        }
        other => other.clone(),
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

fn local_model_fingerprint(config: &serde_json::Value) -> Option<serde_json::Value> {
    let model_path = config.get("model_path").and_then(|value| value.as_str())?;
    let model_path = PathBuf::from(model_path);
    let file_path = if model_path.is_file() {
        model_path
    } else {
        let file_name = config.get("gguf_file").and_then(|value| value.as_str())?;
        model_path.join(file_name)
    };
    if !file_path.is_file() {
        return None;
    }
    let size_bytes = fs::metadata(&file_path).ok().map(|metadata| metadata.len());
    let sha256 = checksum_file(&file_path).ok();
    let file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string());
    Some(serde_json::json!({
        "path": file_path.to_string_lossy(),
        "file": file_name,
        "size_bytes": size_bytes,
        "sha256": sha256,
        "quantization": file_name.as_deref().and_then(infer_gguf_quantization)
    }))
}

fn infer_gguf_quantization(file_name: &str) -> Option<String> {
    let stem = file_name
        .strip_suffix(".gguf")
        .or_else(|| file_name.strip_suffix(".GGUF"))
        .unwrap_or(file_name)
        .to_ascii_uppercase();
    for marker in ["UD-Q", "-IQ", "_IQ", ".IQ", " IQ", "-Q", "_Q", ".Q", " Q"] {
        let Some(index) = stem.find(marker) else {
            continue;
        };
        let offset = if marker == "UD-Q" { 3 } else { 1 };
        let candidate = collect_quant_token(&stem[index + offset..]);
        if is_quant_token(&candidate) {
            return Some(candidate);
        }
    }
    let candidate = collect_quant_token(&stem);
    is_quant_token(&candidate).then_some(candidate)
}

fn collect_quant_token(input: &str) -> String {
    input
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect()
}

fn is_quant_token(candidate: &str) -> bool {
    (candidate.starts_with('Q') || candidate.starts_with("IQ"))
        && candidate.chars().any(|ch| ch.is_ascii_digit())
}

fn generation_settings(config: &serde_json::Value, default_max_tokens: u64) -> serde_json::Value {
    let mut settings = serde_json::Map::new();
    settings.insert(
        "temperature".into(),
        serde_json::json!(generation_temperature(config)),
    );
    settings.insert("top_p".into(), serde_json::json!(generation_top_p(config)));
    settings.insert(
        "max_tokens".into(),
        serde_json::json!(generation_max_tokens(config, default_max_tokens)),
    );
    settings.insert(
        "timeout_seconds".into(),
        serde_json::json!(request_timeout_seconds(config)),
    );
    settings.insert(
        "retry_count".into(),
        serde_json::json!(request_retry_count(config)),
    );
    if let Some(seed) = generation_seed(config) {
        settings.insert("seed".into(), serde_json::json!(seed));
    }
    serde_json::Value::Object(settings)
}

fn generation_temperature(config: &serde_json::Value) -> f64 {
    config
        .get("temperature")
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= 2.0)
        .unwrap_or(0.0)
}

fn generation_top_p(config: &serde_json::Value) -> f64 {
    config
        .get("top_p")
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= 1.0)
        .unwrap_or(1.0)
}

fn generation_max_tokens(config: &serde_json::Value, default_value: u64) -> u64 {
    config
        .get("max_tokens")
        .and_then(|value| value.as_u64())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn generation_seed(config: &serde_json::Value) -> Option<i64> {
    config.get("seed").and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|seed| i64::try_from(seed).ok()))
    })
}

fn request_timeout_seconds(config: &serde_json::Value) -> u64 {
    config
        .get("timeout_seconds")
        .and_then(|value| value.as_u64())
        .filter(|value| *value > 0)
        .unwrap_or(120)
        .clamp(1, 3_600)
}

fn request_retry_count(config: &serde_json::Value) -> u64 {
    config
        .get("retry_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(1)
        .clamp(0, 5)
}

fn insert_optional_seed(payload: &mut serde_json::Value, config: &serde_json::Value) {
    let Some(seed) = generation_seed(config) else {
        return;
    };
    if let Some(object) = payload.as_object_mut() {
        object.insert("seed".into(), serde_json::json!(seed));
    }
}

fn provider_error_from_json(json: &serde_json::Value) -> Option<String> {
    let error = json.get("error")?;
    let message = error
        .get("message")
        .or_else(|| error.get("error"))
        .and_then(|value| value.as_str())
        .unwrap_or("provider returned an error");
    let kind = error
        .get("type")
        .or_else(|| error.get("code"))
        .or_else(|| error.get("status"))
        .and_then(|value| value.as_str())
        .unwrap_or("provider_error");
    Some(format!("provider_error: {}: {}", kind, message))
}

fn provider_json_with_retry(
    config: &serde_json::Value,
    is_cancelled: &dyn Fn() -> bool,
    mut send: impl FnMut() -> Result<ProviderHttpResponse, String>,
) -> Result<ProviderJsonResponse, String> {
    let max_attempts = request_retry_count(config) + 1;
    let mut attempt = 0;
    let mut last_retry_after_ms = None;
    let mut last_retry_delay_ms = None;
    loop {
        ensure_not_cancelled(is_cancelled)?;
        attempt += 1;
        match send() {
            Ok(response) => {
                let json: serde_json::Value = match serde_json::from_str(&response.body) {
                    Ok(json) => json,
                    Err(err) => {
                        if let Some(http_error) = provider_http_status_error_with_retry(
                            response.status,
                            &response.body,
                            response.retry_after_ms,
                        ) {
                            if attempt < max_attempts && should_retry_provider_error(&http_error) {
                                let retry_after_ms = retry_after_ms_from_error(&http_error)
                                    .or(response.retry_after_ms);
                                last_retry_after_ms = retry_after_ms;
                                last_retry_delay_ms = Some(sleep_provider_retry(
                                    attempt,
                                    retry_after_ms,
                                    is_cancelled,
                                )?);
                                continue;
                            }
                            return Err(provider_error_with_retry_metrics(
                                format_provider_http_error_with_transport(http_error, &response),
                                attempt,
                                last_retry_after_ms,
                                last_retry_delay_ms,
                            ));
                        }
                        return Err(provider_error_with_retry_metrics(
                            format_provider_http_error_with_transport(
                                format!("malformed_response: invalid provider JSON: {}", err),
                                &response,
                            ),
                            attempt,
                            last_retry_after_ms,
                            last_retry_delay_ms,
                        ));
                    }
                };
                if let Some(err) = provider_error_from_json(&json) {
                    let err = if let Some(status) = response.status {
                        format_provider_status_prefix(status, response.retry_after_ms, &err)
                    } else {
                        err
                    };
                    if attempt < max_attempts && should_retry_provider_error(&err) {
                        let retry_after_ms =
                            retry_after_ms_from_error(&err).or(response.retry_after_ms);
                        last_retry_after_ms = retry_after_ms;
                        last_retry_delay_ms =
                            Some(sleep_provider_retry(attempt, retry_after_ms, is_cancelled)?);
                        continue;
                    }
                    return Err(provider_error_with_retry_metrics(
                        format_provider_http_error_with_transport(err, &response),
                        attempt,
                        last_retry_after_ms,
                        last_retry_delay_ms,
                    ));
                }
                if let Some(err) = provider_http_status_error_with_retry(
                    response.status,
                    &response.body,
                    response.retry_after_ms,
                ) {
                    if attempt < max_attempts && should_retry_provider_error(&err) {
                        let retry_after_ms =
                            retry_after_ms_from_error(&err).or(response.retry_after_ms);
                        last_retry_after_ms = retry_after_ms;
                        last_retry_delay_ms =
                            Some(sleep_provider_retry(attempt, retry_after_ms, is_cancelled)?);
                        continue;
                    }
                    return Err(provider_error_with_retry_metrics(
                        format_provider_http_error_with_transport(err, &response),
                        attempt,
                        last_retry_after_ms,
                        last_retry_delay_ms,
                    ));
                }
                return Ok(ProviderJsonResponse {
                    json,
                    raw: response.body,
                    attempts: attempt,
                    http_status: response.status,
                    retry_after_ms: response.retry_after_ms.or(last_retry_after_ms),
                    retry_delay_ms: last_retry_delay_ms,
                    time_to_first_byte_ms: response.time_to_first_byte_ms,
                    time_to_first_token_ms: None,
                    request_total_ms: response.request_total_ms,
                });
            }
            Err(err) => {
                if attempt < max_attempts && should_retry_provider_error(&err) {
                    let retry_after_ms = retry_after_ms_from_error(&err);
                    last_retry_after_ms = retry_after_ms;
                    last_retry_delay_ms =
                        Some(sleep_provider_retry(attempt, retry_after_ms, is_cancelled)?);
                    continue;
                }
                return Err(provider_error_with_retry_metrics(
                    err,
                    attempt,
                    last_retry_after_ms,
                    last_retry_delay_ms,
                ));
            }
        }
    }
}

fn provider_stream_with_retry(
    config: &serde_json::Value,
    is_cancelled: &dyn Fn() -> bool,
    mut send: impl FnMut() -> Result<ProviderStreamResponse, String>,
) -> Result<ProviderStreamResponse, String> {
    let max_attempts = request_retry_count(config) + 1;
    let mut attempt = 0;
    let mut last_retry_after_ms = None;
    let mut last_retry_delay_ms = None;
    loop {
        ensure_not_cancelled(is_cancelled)?;
        attempt += 1;
        match send() {
            Ok(mut response) => {
                if response.content.trim().is_empty() {
                    let err = "malformed_response: provider stream did not include text content"
                        .to_string();
                    if attempt < max_attempts && should_retry_provider_error(&err) {
                        last_retry_delay_ms =
                            Some(sleep_provider_retry(attempt, None, is_cancelled)?);
                        continue;
                    }
                    return Err(provider_error_with_retry_metrics(
                        err,
                        attempt,
                        last_retry_after_ms,
                        last_retry_delay_ms,
                    ));
                }
                response.attempts = attempt;
                response.retry_after_ms = response.retry_after_ms.or(last_retry_after_ms);
                response.retry_delay_ms = response.retry_delay_ms.or(last_retry_delay_ms);
                return Ok(response);
            }
            Err(err) => {
                if attempt < max_attempts && should_retry_provider_error(&err) {
                    let retry_after_ms = retry_after_ms_from_error(&err);
                    last_retry_after_ms = retry_after_ms;
                    last_retry_delay_ms =
                        Some(sleep_provider_retry(attempt, retry_after_ms, is_cancelled)?);
                    continue;
                }
                return Err(provider_error_with_retry_metrics(
                    err,
                    attempt,
                    last_retry_after_ms,
                    last_retry_delay_ms,
                ));
            }
        }
    }
}

fn streaming_enabled(adapter: &adapters::AdapterSpec, config: &serde_json::Value) -> bool {
    if let Some(enabled) = config
        .get("streaming")
        .or_else(|| config.get("streaming_enabled"))
        .and_then(|value| value.as_bool())
    {
        return enabled;
    }
    adapter
        .capabilities
        .get("streaming")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn stream_usage_enabled(adapter: &adapters::AdapterSpec, config: &serde_json::Value) -> bool {
    if let Some(enabled) = config.get("stream_usage").and_then(|value| value.as_bool()) {
        return enabled;
    }
    adapter
        .capabilities
        .get("token_usage_reporting")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn should_fallback_streaming_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    if lower.contains("cancelled")
        || lower.contains("auth")
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("rate_limit")
        || lower.contains("429")
        || lower.contains("timeout")
    {
        return false;
    }
    lower.contains("stream")
        || lower.contains("unsupported")
        || lower.contains("unrecognized")
        || lower.contains("unknown parameter")
        || lower.contains("invalid request")
        || lower.contains("malformed_response")
        || lower.contains("http_status 400")
}

fn streaming_payload(payload: &serde_json::Value) -> serde_json::Value {
    let mut payload = payload.clone();
    if let Some(object) = payload.as_object_mut() {
        object.insert("stream".into(), serde_json::json!(true));
    }
    payload
}

fn openai_chat_stream_payload(
    payload: &serde_json::Value,
    adapter: &adapters::AdapterSpec,
    config: &serde_json::Value,
) -> serde_json::Value {
    let mut payload = streaming_payload(payload);
    if stream_usage_enabled(adapter, config) {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "stream_options".into(),
                serde_json::json!({"include_usage": true}),
            );
        }
    }
    payload
}

fn stream_prompt_output(provider: ProviderStreamResponse) -> ModelClientOutput {
    let content = provider.content.clone();
    let raw_sse = provider.raw.clone();
    let mut metrics = provider.metrics.clone();
    metrics.insert(
        "provider_attempts".into(),
        serde_json::json!(provider.attempts),
    );
    insert_provider_stream_transport_metrics(&mut metrics, &provider);
    let raw_response = serde_json::to_string_pretty(&serde_json::json!({
        "stream": true,
        "content": content.clone(),
        "metrics": metrics.clone(),
        "raw_sse": raw_sse
    }))
    .unwrap_or_default();
    ModelClientOutput {
        content,
        raw_response: Some(raw_response),
        metrics,
    }
}

fn should_retry_provider_error(error: &str) -> bool {
    matches!(
        normalize_provider_error_code(error),
        "timeout" | "rate_limit" | "network" | "server_error"
    )
}

fn provider_http_status_error_with_retry(
    status: Option<u16>,
    body: &str,
    retry_after_ms: Option<u64>,
) -> Option<String> {
    let status = status?;
    if (200..300).contains(&status) {
        return None;
    }
    let label = match status {
        401 | 403 => "auth",
        404 => "model_not_found",
        408 => "timeout",
        409 => "provider_failed",
        413 | 422 => "context_overflow",
        429 => "rate_limit",
        500..=599 => "server_error",
        _ => "provider_failed",
    };
    let detail = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("provider returned non-success HTTP status");
    Some(format_provider_status_prefix(
        status,
        retry_after_ms,
        &format!("{}: {}", label, truncate_error_detail(detail, 240)),
    ))
}

fn format_provider_status_prefix(status: u16, retry_after_ms: Option<u64>, detail: &str) -> String {
    let retry_after = retry_after_ms
        .map(|value| format!(" retry_after_ms {}", value))
        .unwrap_or_default();
    format!("http_status {}{}: {}", status, retry_after, detail)
}

fn format_provider_http_error_with_transport(
    error: String,
    response: &ProviderHttpResponse,
) -> String {
    let mut parts = Vec::new();
    if http_status_from_error(&error).is_none() {
        if let Some(status) = response.status {
            parts.push(format!("http_status {}", status));
        }
    }
    if retry_after_ms_from_error(&error).is_none() {
        if let Some(value) = response.retry_after_ms {
            parts.push(format!("retry_after_ms {}", value));
        }
    }
    if let Some(value) = response.time_to_first_byte_ms {
        parts.push(format!("provider_time_to_first_byte_ms {}", value));
    }
    if let Some(value) = response.request_total_ms {
        parts.push(format!("provider_request_total_ms {}", value));
    }
    if parts.is_empty() {
        error
    } else {
        format!("{}: {}", parts.join(" "), error)
    }
}

fn format_provider_stream_error_with_transport(
    error: String,
    response: &ProviderStreamResponse,
) -> String {
    let mut parts = Vec::new();
    if let Some(status) = response.http_status {
        parts.push(format!("http_status {}", status));
    }
    if retry_after_ms_from_error(&error).is_none() {
        if let Some(value) = response.retry_after_ms {
            parts.push(format!("retry_after_ms {}", value));
        }
    }
    if let Some(value) = response.time_to_first_byte_ms {
        parts.push(format!("provider_time_to_first_byte_ms {}", value));
    }
    if let Some(value) = response.time_to_first_token_ms {
        parts.push(format!("provider_time_to_first_token_ms {}", value));
    }
    if let Some(value) = response.request_total_ms {
        parts.push(format!("provider_request_total_ms {}", value));
    }
    if parts.is_empty() {
        error
    } else {
        format!("{}: {}", parts.join(" "), error)
    }
}

fn truncate_error_detail(detail: &str, max_chars: usize) -> String {
    let mut truncated = String::new();
    for (index, ch) in detail.chars().enumerate() {
        if index >= max_chars {
            truncated.push_str("...");
            return truncated;
        }
        truncated.push(ch);
    }
    truncated
}

fn sleep_provider_retry(
    attempt: u64,
    retry_after_ms: Option<u64>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<u64, String> {
    let delay_ms = provider_retry_delay_ms(attempt, retry_after_ms);
    let mut slept = 0;
    while slept < delay_ms {
        ensure_not_cancelled(is_cancelled)?;
        let step = (delay_ms - slept).min(50);
        std::thread::sleep(Duration::from_millis(step));
        slept += step;
    }
    ensure_not_cancelled(is_cancelled)?;
    Ok(delay_ms)
}

fn provider_retry_delay_ms(attempt: u64, retry_after_ms: Option<u64>) -> u64 {
    retry_after_ms
        .map(|delay| delay.min(MAX_PROVIDER_RETRY_AFTER_MS))
        .unwrap_or_else(|| 250 * 2_u64.pow((attempt.saturating_sub(1)).min(3) as u32))
}

fn provider_error_with_retry_metrics(
    error: String,
    attempts: u64,
    retry_after_ms: Option<u64>,
    retry_delay_ms: Option<u64>,
) -> String {
    let mut parts = vec![format!("provider_attempts {}", attempts)];
    if retry_after_ms_from_error(&error).is_none() {
        if let Some(value) = retry_after_ms {
            parts.push(format!("retry_after_ms {}", value));
        }
    }
    if let Some(value) = retry_delay_ms {
        parts.push(format!("provider_retry_delay_ms {}", value));
    }
    format!("{}: {}", parts.join(" "), error)
}

fn retry_after_ms_from_error(error: &str) -> Option<u64> {
    unsigned_after_marker(error, "retry_after_ms ")
}

fn retry_delay_ms_from_error(error: &str) -> Option<u64> {
    unsigned_after_marker(error, "provider_retry_delay_ms ")
}

fn provider_attempts_from_error(error: &str) -> Option<u64> {
    unsigned_after_marker(error, "provider_attempts ")
}

fn http_status_from_error(error: &str) -> Option<u16> {
    unsigned_after_marker(error, "http_status ").and_then(|value| u16::try_from(value).ok())
}

fn float_after_marker(error: &str, marker: &str) -> Option<f64> {
    let start = error.find(marker)? + marker.len();
    let number: String = error[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    number.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn unsigned_after_marker(error: &str, marker: &str) -> Option<u64> {
    let start = error.find(marker)? + marker.len();
    let digits: String = error[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn provider_error_transport_metrics(error: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut metrics = serde_json::Map::new();
    if let Some(attempts) = provider_attempts_from_error(error) {
        metrics.insert("provider_attempts".into(), serde_json::json!(attempts));
    }
    if let Some(status) = http_status_from_error(error) {
        metrics.insert("http_status".into(), serde_json::json!(status));
    }
    if let Some(retry_after_ms) = retry_after_ms_from_error(error) {
        metrics.insert(
            "provider_retry_after_ms".into(),
            serde_json::json!(retry_after_ms),
        );
    }
    if let Some(retry_delay_ms) = retry_delay_ms_from_error(error) {
        metrics.insert(
            "provider_retry_delay_ms".into(),
            serde_json::json!(retry_delay_ms),
        );
    }
    if let Some(value) = float_after_marker(error, "provider_time_to_first_byte_ms ") {
        metrics.insert(
            "provider_time_to_first_byte_ms".into(),
            serde_json::json!(value),
        );
    }
    if let Some(value) = float_after_marker(error, "provider_time_to_first_token_ms ") {
        metrics.insert(
            "provider_time_to_first_token_ms".into(),
            serde_json::json!(value),
        );
    }
    if let Some(value) = float_after_marker(error, "provider_request_total_ms ") {
        metrics.insert("provider_request_total_ms".into(), serde_json::json!(value));
    }
    metrics
}

fn target_execution_error_code(target: &store::TargetRecord, error: &str) -> &'static str {
    match target.kind.as_str() {
        "direct_model" | "harnessed_model" => {
            let lower = error.to_lowercase();
            if lower.contains("model output did not match")
                || lower.contains("model patch did not apply")
                || lower.contains("edit missing")
            {
                "model_output_invalid"
            } else {
                normalize_provider_error_code(error)
            }
        }
        "cli_agent" => {
            if error.to_lowercase().contains("timeout") {
                "timeout"
            } else {
                "cli_agent_failed"
            }
        }
        "mock" => "mock_failed",
        _ => "target_execution_failed",
    }
}

fn normalize_provider_error_code(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.contains("cancelled") {
        return "cancelled";
    }
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("deadline")
        || lower.contains("provider call exceeded")
    {
        return "timeout";
    }
    if lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("invalid api key")
        || lower.contains("api key")
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("permission")
    {
        return "auth";
    }
    if lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
        || lower.contains("quota")
        || lower.contains("429")
    {
        return "rate_limit";
    }
    if lower.contains("model_not_found")
        || lower.contains("model not found")
        || lower.contains("unknown model")
        || lower.contains("does not exist")
        || lower.contains("404")
    {
        return "model_not_found";
    }
    if lower.contains("context length")
        || lower.contains("context_length")
        || lower.contains("maximum context")
        || lower.contains("context window")
        || lower.contains("too many tokens")
        || lower.contains("token limit")
    {
        return "context_overflow";
    }
    if lower.contains("content filter")
        || lower.contains("content_filter")
        || lower.contains("safety")
        || lower.contains("blocked")
        || lower.contains("policy")
    {
        return "content_filter";
    }
    if lower.contains("malformed_response")
        || lower.contains("invalid provider json")
        || lower.contains("did not include")
    {
        return "malformed_response";
    }
    if lower.contains("connection refused")
        || lower.contains("failed to connect")
        || lower.contains("could not resolve")
        || lower.contains("name or service not known")
        || lower.contains("network")
        || lower.contains("curl")
    {
        return "network";
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
    "provider_failed"
}

fn score_prompt_response(scoring: &ScoringSpec, content: &str) -> PromptScore {
    let mut checks = Vec::new();
    let mut passed = 0usize;
    let mut total = 0usize;
    let normalized = content.to_lowercase();

    if let Some(expected) = &scoring.expect_exact {
        total += 1;
        let ok = content.trim() == expected.trim();
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "exact",
            "expected": expected,
            "passed": ok
        }));
    }

    for expected in &scoring.expect_contains {
        total += 1;
        let ok = normalized.contains(&expected.to_lowercase());
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "contains",
            "expected": expected,
            "passed": ok
        }));
    }

    for pattern in &scoring.expect_regex {
        total += 1;
        let compiled = Regex::new(pattern);
        let ok = compiled
            .as_ref()
            .map(|regex| regex.is_match(content.trim()))
            .unwrap_or(false);
        if ok {
            passed += 1;
        }
        let mut check = serde_json::json!({
            "kind": "regex",
            "pattern": pattern,
            "passed": ok
        });
        if let Err(err) = compiled {
            check["error"] = serde_json::json!(err.to_string());
        }
        checks.push(check);
    }

    for forbidden in &scoring.expect_not_contains {
        total += 1;
        let ok = !normalized.contains(&forbidden.to_lowercase());
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "not_contains",
            "expected_absent": forbidden,
            "passed": ok
        }));
    }

    let parsed_json = if scoring_requires_json(scoring) {
        parse_json_response(content)
    } else {
        None
    };
    if scoring.expect_json {
        total += 1;
        let ok = parsed_json.is_some();
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "json_valid",
            "passed": ok
        }));
    }
    for (path, expected) in &scoring.json_field_equals {
        total += 1;
        let actual = parsed_json
            .as_ref()
            .and_then(|json| json_field(json, path))
            .cloned();
        let ok = actual.as_ref() == Some(expected);
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "json_field_equals",
            "path": path,
            "expected": expected,
            "actual": actual,
            "passed": ok
        }));
    }
    for (path, expected_values) in &scoring.json_field_contains {
        for expected in expected_values {
            total += 1;
            let actual = parsed_json
                .as_ref()
                .and_then(|json| json_field(json, path))
                .cloned();
            let ok = actual
                .as_ref()
                .map(|value| json_value_contains(value, expected))
                .unwrap_or(false);
            if ok {
                passed += 1;
            }
            checks.push(serde_json::json!({
                "kind": "json_field_contains",
                "path": path,
                "expected": expected,
                "actual": actual,
                "passed": ok
            }));
        }
    }
    for (path, expected_values) in &scoring.json_field_array_exact {
        total += 1;
        let actual = parsed_json
            .as_ref()
            .and_then(|json| json_field(json, path))
            .cloned();
        let (ok, missing, unexpected) = actual
            .as_ref()
            .map(|value| json_array_exact_unordered(value, expected_values))
            .unwrap_or_else(|| (false, expected_values.clone(), Vec::new()));
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "json_field_array_exact",
            "path": path,
            "expected": expected_values,
            "actual": actual,
            "missing": missing,
            "unexpected": unexpected,
            "passed": ok
        }));
    }
    for (path, expected_values) in &scoring.json_field_array_exact_ordered {
        total += 1;
        let actual = parsed_json
            .as_ref()
            .and_then(|json| json_field(json, path))
            .cloned();
        let (ok, first_mismatch_index, actual_length) = actual
            .as_ref()
            .map(|value| json_array_exact_ordered(value, expected_values))
            .unwrap_or((false, None, None));
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "json_field_array_exact_ordered",
            "path": path,
            "expected": expected_values,
            "actual": actual,
            "expected_length": expected_values.len(),
            "actual_length": actual_length,
            "first_mismatch_index": first_mismatch_index,
            "passed": ok
        }));
    }
    for (path, expected_keys) in &scoring.json_field_object_keys_exact {
        total += 1;
        let actual = parsed_json
            .as_ref()
            .and_then(|json| json_field_or_root(json, path))
            .cloned();
        let (ok, missing, unexpected, actual_keys) = actual
            .as_ref()
            .map(|value| json_object_keys_exact(value, expected_keys))
            .unwrap_or_else(|| {
                (
                    false,
                    expected_keys.clone(),
                    Vec::new(),
                    Option::<Vec<String>>::None,
                )
            });
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "json_field_object_keys_exact",
            "path": path,
            "expected": expected_keys,
            "actual": actual,
            "actual_keys": actual_keys,
            "missing": missing,
            "unexpected": unexpected,
            "passed": ok
        }));
    }
    for (path, expected) in &scoring.json_field_number_close {
        total += 1;
        let actual = parsed_json
            .as_ref()
            .and_then(|json| json_field(json, path))
            .and_then(json_value_as_f64);
        let tolerance = expected.tolerance.max(0.0);
        let delta = actual.map(|value| (value - expected.expected).abs());
        let ok = delta.map(|value| value <= tolerance).unwrap_or(false);
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "json_field_number_close",
            "path": path,
            "expected": expected.expected,
            "tolerance": tolerance,
            "actual": actual,
            "delta": delta,
            "passed": ok
        }));
    }
    for (path, bounds) in &scoring.json_field_number_bounds {
        total += 1;
        let actual = parsed_json
            .as_ref()
            .and_then(|json| json_field(json, path))
            .and_then(json_value_as_f64);
        let has_bound = bounds.min.is_some() || bounds.max.is_some();
        let min_passed = bounds
            .min
            .map(|min| actual.is_some_and(|value| value >= min));
        let max_passed = bounds
            .max
            .map(|max| actual.is_some_and(|value| value <= max));
        let ok = has_bound
            && actual.is_some()
            && min_passed.unwrap_or(true)
            && max_passed.unwrap_or(true);
        if ok {
            passed += 1;
        }
        checks.push(serde_json::json!({
            "kind": "json_field_number_bounds",
            "path": path,
            "min": bounds.min,
            "max": bounds.max,
            "actual": actual,
            "min_passed": min_passed,
            "max_passed": max_passed,
            "passed": ok
        }));
    }

    if total == 0 {
        total = 1;
        let ok = !content.trim().is_empty();
        if ok {
            passed = 1;
        }
        checks.push(serde_json::json!({
            "kind": "non_empty",
            "passed": ok
        }));
    }

    let score = passed as f64 / total as f64;
    let status = if passed == total { "passed" } else { "failed" }.to_string();
    let error_message = if status == "passed" {
        None
    } else {
        Some(prompt_failure_message(&checks))
    };
    PromptScore {
        status: status.clone(),
        score,
        tests: serde_json::json!({
            "passed": passed,
            "total": total,
            "checks": checks
        }),
        error_message,
    }
}

fn scoring_requires_json(scoring: &ScoringSpec) -> bool {
    scoring.expect_json
        || !scoring.json_field_equals.is_empty()
        || !scoring.json_field_contains.is_empty()
        || !scoring.json_field_object_keys_exact.is_empty()
        || !scoring.json_field_array_exact.is_empty()
        || !scoring.json_field_array_exact_ordered.is_empty()
        || !scoring.json_field_number_close.is_empty()
        || !scoring.json_field_number_bounds.is_empty()
}

fn prompt_failure_message(checks: &[serde_json::Value]) -> String {
    const MAX_FAILED_CHECKS: usize = 5;
    let mut labels = Vec::new();
    let mut failed = 0usize;
    for check in checks {
        if check.get("passed").and_then(|value| value.as_bool()) == Some(false) {
            failed += 1;
            if labels.len() < MAX_FAILED_CHECKS {
                labels.push(prompt_check_label(check));
            }
        }
    }

    if labels.is_empty() {
        return "prompt expectations failed".into();
    }

    let mut message = format!("prompt expectations failed: {}", labels.join(", "));
    if failed > labels.len() {
        message.push_str(&format!("; +{} more", failed - labels.len()));
    }
    message
}

fn prompt_check_label(check: &serde_json::Value) -> String {
    let kind = check
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("check");
    match check
        .get("path")
        .and_then(|value| value.as_str())
        .map(prompt_path_label)
        .filter(|path| !path.is_empty())
    {
        Some(path) => format!("{kind}({path})"),
        None => kind.to_string(),
    }
}

fn prompt_path_label(path: &str) -> String {
    const MAX_PATH_CHARS: usize = 48;
    let trimmed = path.trim();
    let mut label = trimmed.chars().take(MAX_PATH_CHARS).collect::<String>();
    if trimmed.chars().count() > MAX_PATH_CHARS {
        label.push_str("...");
    }
    label
}

fn prompt_failure_error_code(tests: &serde_json::Value) -> Option<&'static str> {
    let total = tests.get("total").and_then(|value| value.as_u64())?;
    let passed = tests.get("passed").and_then(|value| value.as_u64())?;
    if passed >= total {
        return None;
    }
    let structured_failure = tests
        .get("checks")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .any(|check| {
            check.get("passed").and_then(|value| value.as_bool()) == Some(false)
                && check
                    .get("kind")
                    .and_then(|value| value.as_str())
                    .is_some_and(prompt_check_kind_is_structured)
        });
    Some(if structured_failure {
        "invalid_output_format"
    } else {
        "test_failed"
    })
}

fn prompt_check_kind_is_structured(kind: &str) -> bool {
    matches!(
        kind,
        "json_valid"
            | "json_field_equals"
            | "json_field_contains"
            | "json_field_array_exact"
            | "json_field_array_exact_ordered"
            | "json_field_object_keys_exact"
            | "json_field_number_close"
            | "json_field_number_bounds"
    )
}

fn parse_json_response(content: &str) -> Option<serde_json::Value> {
    let trimmed = content.trim();
    if let Ok(json) = serde_json::from_str(trimmed) {
        return Some(json);
    }
    let unfenced = strip_code_fence(trimmed);
    if let Ok(json) = serde_json::from_str(unfenced) {
        return Some(json);
    }
    let object =
        extract_json_like(unfenced, '{', '}').or_else(|| extract_json_like(unfenced, '[', ']'))?;
    serde_json::from_str(object).ok()
}

fn strip_code_fence(content: &str) -> &str {
    if !content.starts_with("```") {
        return content;
    }
    let Some(first_newline) = content.find('\n') else {
        return content;
    };
    let body = &content[first_newline + 1..];
    body.rsplit_once("```")
        .map(|(before, _)| before.trim())
        .unwrap_or(body.trim())
}

fn extract_json_like(content: &str, open: char, close: char) -> Option<&str> {
    let start = content.find(open)?;
    let end = content.rfind(close)?;
    (end >= start).then(|| &content[start..=end])
}

fn json_field<'a>(json: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    if path.starts_with('/') {
        return json.pointer(path);
    }
    let mut current = json;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn json_field_or_root<'a>(
    json: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    if path.trim().is_empty() || path == "$" {
        Some(json)
    } else {
        json_field(json, path)
    }
}

fn json_value_contains(value: &serde_json::Value, expected: &str) -> bool {
    let expected = expected.trim();
    if expected.is_empty() {
        return false;
    }
    let expected_lower = expected.to_lowercase();
    match value {
        serde_json::Value::String(actual) => actual.to_lowercase().contains(&expected_lower),
        serde_json::Value::Number(actual) => actual.to_string() == expected,
        serde_json::Value::Bool(actual) => actual.to_string() == expected_lower,
        serde_json::Value::Array(items) => {
            items.iter().any(|item| json_value_contains(item, expected))
        }
        serde_json::Value::Object(_) => serde_json::to_string(value)
            .map(|actual| actual.to_lowercase().contains(&expected_lower))
            .unwrap_or(false),
        serde_json::Value::Null => false,
    }
}

fn json_object_keys_exact(
    value: &serde_json::Value,
    expected: &[String],
) -> (bool, Vec<String>, Vec<String>, Option<Vec<String>>) {
    let Some(object) = value.as_object() else {
        return (false, expected.to_vec(), Vec::new(), None);
    };
    let expected_keys = expected.iter().cloned().collect::<BTreeSet<_>>();
    let actual_keys = object.keys().cloned().collect::<BTreeSet<_>>();
    let missing = set_difference_strings(&expected_keys, &actual_keys);
    let unexpected = set_difference_strings(&actual_keys, &expected_keys);
    let ok = missing.is_empty() && unexpected.is_empty();
    (
        ok,
        missing,
        unexpected,
        Some(actual_keys.into_iter().collect()),
    )
}

fn set_difference_strings(all: &BTreeSet<String>, present: &BTreeSet<String>) -> Vec<String> {
    all.difference(present).cloned().collect()
}

fn json_array_exact_unordered(
    value: &serde_json::Value,
    expected: &[serde_json::Value],
) -> (bool, Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let Some(actual_items) = value.as_array() else {
        return (false, expected.to_vec(), vec![value.clone()]);
    };
    let mut unmatched_actual = actual_items.clone();
    let mut missing = Vec::new();
    for expected_item in expected {
        if let Some(index) = unmatched_actual
            .iter()
            .position(|actual| json_values_equivalent(actual, expected_item))
        {
            unmatched_actual.remove(index);
        } else {
            missing.push(expected_item.clone());
        }
    }
    let ok = missing.is_empty() && unmatched_actual.is_empty();
    (ok, missing, unmatched_actual)
}

fn json_array_exact_ordered(
    value: &serde_json::Value,
    expected: &[serde_json::Value],
) -> (bool, Option<usize>, Option<usize>) {
    let Some(actual_items) = value.as_array() else {
        return (false, Some(0), None);
    };
    let first_mismatch = actual_items
        .iter()
        .zip(expected.iter())
        .position(|(actual, expected)| !json_values_equivalent(actual, expected))
        .or_else(|| {
            (actual_items.len() != expected.len()).then_some(actual_items.len().min(expected.len()))
        });
    (
        first_mismatch.is_none(),
        first_mismatch,
        Some(actual_items.len()),
    )
}

fn json_values_equivalent(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    match (actual, expected) {
        (serde_json::Value::Number(actual), serde_json::Value::Number(expected)) => actual
            .as_f64()
            .zip(expected.as_f64())
            .map(|(left, right)| (left - right).abs() <= f64::EPSILON)
            .unwrap_or_else(|| actual == expected),
        _ => actual == expected,
    }
}

fn json_value_as_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

struct StreamParseState {
    format: StreamFormat,
    content: String,
    raw: String,
    metrics: serde_json::Map<String, serde_json::Value>,
    event_name: Option<String>,
    event_data: Vec<String>,
    event_started_ms: Option<f64>,
    error: Option<String>,
    http_status: Option<u16>,
    time_to_first_byte_ms: Option<f64>,
    time_to_first_token_ms: Option<f64>,
    request_total_ms: Option<f64>,
}

impl StreamParseState {
    fn new(format: StreamFormat) -> Self {
        Self {
            format,
            content: String::new(),
            raw: String::new(),
            metrics: serde_json::Map::new(),
            event_name: None,
            event_data: Vec::new(),
            event_started_ms: None,
            error: None,
            http_status: None,
            time_to_first_byte_ms: None,
            time_to_first_token_ms: None,
            request_total_ms: None,
        }
    }

    fn handle_stdout_line(&mut self, line: &str, elapsed_ms: f64) {
        self.raw.push_str(line);
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if self.handle_curl_marker(trimmed) {
            return;
        }
        if trimmed.is_empty() {
            self.dispatch_event(elapsed_ms);
            return;
        }
        if let Some(value) = trimmed.strip_prefix("event:") {
            self.event_name = Some(value.trim().to_string());
            return;
        }
        if let Some(value) = trimmed.strip_prefix("data:") {
            self.event_started_ms.get_or_insert(elapsed_ms);
            self.event_data.push(value.trim_start().to_string());
        }
    }

    fn finish(mut self, elapsed_ms: f64) -> (ProviderStreamResponse, Option<String>) {
        self.dispatch_event(elapsed_ms);
        let response = ProviderStreamResponse {
            content: self.content,
            raw: self.raw,
            metrics: self.metrics,
            attempts: 0,
            http_status: self.http_status,
            retry_after_ms: None,
            retry_delay_ms: None,
            time_to_first_byte_ms: self.time_to_first_byte_ms,
            time_to_first_token_ms: self.time_to_first_token_ms,
            request_total_ms: self.request_total_ms,
        };
        (response, self.error)
    }

    fn handle_curl_marker(&mut self, line: &str) -> bool {
        if let Some(value) = line.strip_prefix("__BENCHFORGE_HTTP_STATUS__:") {
            self.http_status = value
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|status| *status > 0);
            return true;
        }
        if let Some(value) = line.strip_prefix("__BENCHFORGE_TIME_STARTTRANSFER__:") {
            self.time_to_first_byte_ms = parse_curl_seconds_to_ms(value);
            return true;
        }
        if let Some(value) = line.strip_prefix("__BENCHFORGE_TIME_TOTAL__:") {
            self.request_total_ms = parse_curl_seconds_to_ms(value);
            return true;
        }
        false
    }

    fn dispatch_event(&mut self, elapsed_ms: f64) {
        if self.event_data.is_empty() {
            self.event_name = None;
            self.event_started_ms = None;
            return;
        }
        let event_name = self.event_name.take();
        let data = self.event_data.join("\n");
        let event_ms = self.event_started_ms.take().unwrap_or(elapsed_ms);
        self.event_data.clear();
        if data.trim() == "[DONE]" {
            return;
        }
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) else {
            return;
        };
        match self.format {
            StreamFormat::OpenAiChat => self.handle_openai_chat_event(&json, event_ms),
            StreamFormat::OpenAiResponses => {
                self.handle_openai_responses_event(event_name.as_deref(), &json, event_ms)
            }
            StreamFormat::AnthropicMessages => {
                self.handle_anthropic_event(event_name.as_deref(), &json, event_ms)
            }
        }
    }

    fn handle_openai_chat_event(&mut self, json: &serde_json::Value, elapsed_ms: f64) {
        if let Some(err) = provider_error_from_json(json) {
            self.error = Some(err);
            return;
        }
        if let Some(model) = json.get("model").and_then(|value| value.as_str()) {
            self.metrics
                .insert("provider_model".into(), serde_json::json!(model));
        }
        if let Some(usage) = json.get("usage") {
            insert_openai_usage_metrics(&mut self.metrics, usage);
        }
        let Some(choices) = json.get("choices").and_then(|value| value.as_array()) else {
            return;
        };
        for choice in choices {
            if let Some(reason) = choice.get("finish_reason").and_then(|value| value.as_str()) {
                self.metrics
                    .insert("finish_reason".into(), serde_json::json!(reason));
            }
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            for field in ["content", "refusal"] {
                if let Some(text) = delta
                    .get(field)
                    .and_then(|value| value.as_str())
                    .filter(|text| !text.is_empty())
                {
                    self.record_text_delta(text, elapsed_ms);
                }
            }
        }
    }

    fn handle_openai_responses_event(
        &mut self,
        event_name: Option<&str>,
        json: &serde_json::Value,
        elapsed_ms: f64,
    ) {
        if let Some(err) = provider_error_from_json(json) {
            self.error = Some(err);
            return;
        }
        let event_type = json
            .get("type")
            .and_then(|value| value.as_str())
            .or(event_name)
            .unwrap_or_default();
        match event_type {
            "response.output_text.delta" | "response.refusal.delta" => {
                if let Some(text) = json
                    .get("delta")
                    .and_then(|value| value.as_str())
                    .filter(|text| !text.is_empty())
                {
                    self.record_text_delta(text, elapsed_ms);
                }
            }
            "response.completed" => {
                if let Some(response) = json.get("response") {
                    merge_metrics(&mut self.metrics, openai_responses_metrics(response));
                    if self.content.trim().is_empty() {
                        if let Some(text) = openai_responses_text(response) {
                            self.record_text_delta(&text, elapsed_ms);
                        }
                    }
                }
            }
            "response.failed" => {
                if let Some(response) = json.get("response") {
                    self.error = response
                        .get("error")
                        .and_then(|error| {
                            let code = error
                                .get("code")
                                .and_then(|value| value.as_str())
                                .unwrap_or("provider_error");
                            let message = error
                                .get("message")
                                .and_then(|value| value.as_str())
                                .unwrap_or("provider stream failed");
                            Some(format!("provider_error: {}: {}", code, message))
                        })
                        .or_else(|| Some("provider_error: response stream failed".to_string()));
                }
            }
            "response.incomplete" => {
                self.metrics
                    .insert("finish_reason".into(), serde_json::json!("incomplete"));
            }
            "error" => {
                self.error = Some(stream_error_message(json, "provider stream error"));
            }
            _ => {}
        }
    }

    fn handle_anthropic_event(
        &mut self,
        event_name: Option<&str>,
        json: &serde_json::Value,
        elapsed_ms: f64,
    ) {
        let event_type = json
            .get("type")
            .and_then(|value| value.as_str())
            .or(event_name)
            .unwrap_or_default();
        if event_type == "error" {
            self.error = Some(stream_error_message(json, "provider stream error"));
            return;
        }
        match event_type {
            "message_start" => {
                if let Some(message) = json.get("message") {
                    if let Some(model) = message.get("model").and_then(|value| value.as_str()) {
                        self.metrics
                            .insert("provider_model".into(), serde_json::json!(model));
                    }
                    if let Some(usage) = message.get("usage") {
                        insert_anthropic_usage_metrics(&mut self.metrics, usage);
                    }
                }
            }
            "content_block_start" => {
                if let Some(text) = json
                    .get("content_block")
                    .and_then(|block| block.get("text"))
                    .and_then(|value| value.as_str())
                    .filter(|text| !text.is_empty())
                {
                    self.record_text_delta(text, elapsed_ms);
                }
            }
            "content_block_delta" => {
                let Some(delta) = json.get("delta") else {
                    return;
                };
                if delta
                    .get("type")
                    .and_then(|value| value.as_str())
                    .is_some_and(|kind| kind == "text_delta")
                {
                    if let Some(text) = delta
                        .get("text")
                        .and_then(|value| value.as_str())
                        .filter(|text| !text.is_empty())
                    {
                        self.record_text_delta(text, elapsed_ms);
                    }
                }
            }
            "message_delta" => {
                if let Some(delta) = json.get("delta") {
                    if let Some(reason) = delta.get("stop_reason").and_then(|value| value.as_str())
                    {
                        self.metrics
                            .insert("finish_reason".into(), serde_json::json!(reason));
                    }
                }
                if let Some(usage) = json.get("usage") {
                    insert_anthropic_usage_metrics(&mut self.metrics, usage);
                }
            }
            _ => {}
        }
    }

    fn record_text_delta(&mut self, text: &str, elapsed_ms: f64) {
        if self.time_to_first_token_ms.is_none() {
            self.time_to_first_token_ms = Some(elapsed_ms);
        }
        self.content.push_str(text);
    }
}

fn merge_metrics(
    target: &mut serde_json::Map<String, serde_json::Value>,
    source: serde_json::Map<String, serde_json::Value>,
) {
    for (key, value) in source {
        target.insert(key, value);
    }
}

fn insert_openai_usage_metrics(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    usage: &serde_json::Value,
) {
    if let Some(value) = usage_token_count(usage, &["prompt_tokens", "input_tokens"]) {
        metrics.insert("prompt_tokens".into(), serde_json::json!(value));
    }
    if let Some(value) = usage_token_count(usage, &["completion_tokens", "output_tokens"]) {
        metrics.insert("completion_tokens".into(), serde_json::json!(value));
    }
    let total = usage_token_count(usage, &["total_tokens"]).or_else(|| {
        let prompt = metrics
            .get("prompt_tokens")
            .and_then(|value| value.as_u64())?;
        let completion = metrics
            .get("completion_tokens")
            .and_then(|value| value.as_u64())?;
        Some(prompt + completion)
    });
    if let Some(value) = total {
        metrics.insert("total_tokens".into(), serde_json::json!(value));
    }
    if let Some(value) = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .or_else(|| usage.pointer("/output_tokens_details/reasoning_tokens"))
        .or_else(|| usage.get("reasoning_tokens"))
        .and_then(|value| value.as_u64())
    {
        metrics.insert("reasoning_tokens".into(), serde_json::json!(value));
    }
    if let Some(value) = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cached_tokens"))
        .or_else(|| usage.get("cache_read_tokens"))
        .and_then(|value| value.as_u64())
    {
        metrics.insert("cached_tokens".into(), serde_json::json!(value));
        metrics.insert("cache_read_tokens".into(), serde_json::json!(value));
    }
    if let Some(value) = usage
        .pointer("/prompt_tokens_details/cache_write_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
        .or_else(|| usage.get("cache_write_tokens"))
        .and_then(|value| value.as_u64())
    {
        metrics.insert("cache_write_tokens".into(), serde_json::json!(value));
    }
}

fn usage_token_count(usage: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| usage.get(*key).and_then(|value| value.as_u64()))
}

fn insert_anthropic_usage_metrics(
    metrics: &mut serde_json::Map<String, serde_json::Value>,
    usage: &serde_json::Value,
) {
    if let Some(value) = usage.get("input_tokens").and_then(|value| value.as_u64()) {
        metrics.insert("prompt_tokens".into(), serde_json::json!(value));
    }
    if let Some(value) = usage.get("output_tokens").and_then(|value| value.as_u64()) {
        metrics.insert("completion_tokens".into(), serde_json::json!(value));
    }
    let input = metrics
        .get("prompt_tokens")
        .and_then(|value| value.as_u64());
    let output = metrics
        .get("completion_tokens")
        .and_then(|value| value.as_u64());
    if let (Some(input), Some(output)) = (input, output) {
        metrics.insert("total_tokens".into(), serde_json::json!(input + output));
    }
    if let Some(value) = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.get("cache_read_tokens"))
        .and_then(|value| value.as_u64())
    {
        metrics.insert("cached_tokens".into(), serde_json::json!(value));
        metrics.insert("cache_read_tokens".into(), serde_json::json!(value));
    }
    if let Some(value) = usage
        .get("cache_creation_input_tokens")
        .or_else(|| usage.get("cache_write_tokens"))
        .and_then(|value| value.as_u64())
    {
        metrics.insert("cache_write_tokens".into(), serde_json::json!(value));
    }
}

fn stream_error_message(json: &serde_json::Value, fallback: &str) -> String {
    let error = json.get("error").unwrap_or(json);
    let kind = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(|value| value.as_str())
        .unwrap_or("provider_error");
    let message = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback);
    format!("provider_error: {}: {}", kind, message)
}

fn post_json_stream_with_curl(
    url: &str,
    payload: &serde_json::Value,
    headers: &[(&str, String)],
    timeout_seconds: u64,
    format: StreamFormat,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ProviderStreamResponse, String> {
    ensure_not_cancelled(is_cancelled)?;
    if !adapters::command_exists("curl") {
        return Err("provider_skipped: curl is not available".into());
    }
    let body_path = paths::app_data_dir().join(format!("request-{}.json", uuid::Uuid::new_v4()));
    let headers_path =
        paths::app_data_dir().join(format!("response-{}.headers", uuid::Uuid::new_v4()));
    fs::write(
        &body_path,
        serde_json::to_vec(payload).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let mut args = vec![
        "-sS".to_string(),
        "-N".to_string(),
        "--connect-timeout".to_string(),
        "10".to_string(),
        "--max-time".to_string(),
        timeout_seconds.clamp(1, 3_600).to_string(),
        "-X".to_string(),
        "POST".to_string(),
        url.to_string(),
        "-D".to_string(),
        headers_path.to_string_lossy().to_string(),
        "--data-binary".to_string(),
        format!("@{}", body_path.to_string_lossy()),
        "--write-out".to_string(),
        "\n__BENCHFORGE_HTTP_STATUS__:%{http_code}\n__BENCHFORGE_TIME_STARTTRANSFER__:%{time_starttransfer}\n__BENCHFORGE_TIME_TOTAL__:%{time_total}".to_string(),
    ];
    for (name, value) in headers {
        if !value.ends_with(' ') && !value.ends_with("Bearer ") {
            args.push("-H".to_string());
            args.push(format!("{}: {}", name, value));
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let capture = run_streaming_curl_capture(
        command_at(&paths::resource_root(), "curl", &arg_refs),
        Duration::from_secs(timeout_seconds.clamp(1, 3_600)),
        format,
        is_cancelled,
    );
    let headers = fs::read_to_string(&headers_path).unwrap_or_default();
    let _ = fs::remove_file(body_path);
    let _ = fs::remove_file(headers_path);
    let capture = capture?;
    if capture.timed_out {
        return Err(format!(
            "timeout: provider call exceeded {} seconds",
            timeout_seconds.clamp(1, 3_600)
        ));
    }
    if capture.code.unwrap_or(1) != 0 {
        return Err(format!(
            "provider call failed: {}",
            capture.stderr.lines().next().unwrap_or("curl failed")
        ));
    }
    let mut response = capture.response;
    response.retry_after_ms = parse_retry_after_ms(&headers);
    if let Some(err) = provider_http_status_error_with_retry(
        response.http_status,
        &response.raw,
        response.retry_after_ms,
    ) {
        return Err(err);
    }
    if let Some(err) = capture.stream_error {
        return Err(format_provider_stream_error_with_transport(err, &response));
    }
    Ok(response)
}

fn post_json_with_curl(
    url: &str,
    payload: &serde_json::Value,
    headers: &[(&str, String)],
    timeout_seconds: u64,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ProviderHttpResponse, String> {
    ensure_not_cancelled(is_cancelled)?;
    if !adapters::command_exists("curl") {
        return Err("provider_skipped: curl is not available".into());
    }
    let body_path = paths::app_data_dir().join(format!("request-{}.json", uuid::Uuid::new_v4()));
    let headers_path =
        paths::app_data_dir().join(format!("response-{}.headers", uuid::Uuid::new_v4()));
    fs::write(
        &body_path,
        serde_json::to_vec(payload).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    let mut args = vec![
        "-sS".to_string(),
        "--connect-timeout".to_string(),
        "10".to_string(),
        "--max-time".to_string(),
        timeout_seconds.clamp(1, 3_600).to_string(),
        "-X".to_string(),
        "POST".to_string(),
        url.to_string(),
        "-D".to_string(),
        headers_path.to_string_lossy().to_string(),
        "--data-binary".to_string(),
        format!("@{}", body_path.to_string_lossy()),
        "--write-out".to_string(),
        "\n__BENCHFORGE_HTTP_STATUS__:%{http_code}\n__BENCHFORGE_TIME_STARTTRANSFER__:%{time_starttransfer}\n__BENCHFORGE_TIME_TOTAL__:%{time_total}".to_string(),
    ];
    for (name, value) in headers {
        if !value.ends_with(' ') && !value.ends_with("Bearer ") {
            args.push("-H".to_string());
            args.push(format!("{}: {}", name, value));
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let capture = run_command_capture_checked(
        command_at(&paths::resource_root(), "curl", &arg_refs),
        Duration::from_secs(timeout_seconds.clamp(1, 3_600)),
        is_cancelled,
    );
    let headers = fs::read_to_string(&headers_path).unwrap_or_default();
    let _ = fs::remove_file(body_path);
    let _ = fs::remove_file(headers_path);
    let capture = capture?;
    if capture.timed_out {
        return Err(format!(
            "timeout: provider call exceeded {} seconds",
            timeout_seconds.clamp(1, 3_600)
        ));
    }
    if capture.code.unwrap_or(1) != 0 {
        return Err(format!(
            "provider call failed: {}",
            capture.stderr.lines().next().unwrap_or("curl failed")
        ));
    }
    let mut response = parse_curl_http_response(&capture.stdout);
    response.retry_after_ms = parse_retry_after_ms(&headers);
    Ok(response)
}

fn get_json_with_curl(
    url: &str,
    headers: &[(&str, String)],
    timeout_seconds: u64,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ProviderHttpResponse, String> {
    ensure_not_cancelled(is_cancelled)?;
    if !adapters::command_exists("curl") {
        return Err("provider_skipped: curl is not available".into());
    }
    let headers_path =
        paths::app_data_dir().join(format!("response-{}.headers", uuid::Uuid::new_v4()));
    let mut args = vec![
        "-sS".to_string(),
        "--connect-timeout".to_string(),
        timeout_seconds.clamp(1, 5).to_string(),
        "--max-time".to_string(),
        timeout_seconds.clamp(1, 3_600).to_string(),
        "-X".to_string(),
        "GET".to_string(),
        url.to_string(),
        "-D".to_string(),
        headers_path.to_string_lossy().to_string(),
        "--write-out".to_string(),
        "\n__BENCHFORGE_HTTP_STATUS__:%{http_code}\n__BENCHFORGE_TIME_STARTTRANSFER__:%{time_starttransfer}\n__BENCHFORGE_TIME_TOTAL__:%{time_total}".to_string(),
    ];
    for (name, value) in headers {
        if !value.ends_with(' ') && !value.ends_with("Bearer ") {
            args.push("-H".to_string());
            args.push(format!("{}: {}", name, value));
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let capture = run_command_capture_checked(
        command_at(&paths::resource_root(), "curl", &arg_refs),
        Duration::from_secs(timeout_seconds.clamp(1, 3_600)),
        is_cancelled,
    );
    let headers = fs::read_to_string(&headers_path).unwrap_or_default();
    let _ = fs::remove_file(headers_path);
    let capture = capture?;
    if capture.timed_out {
        return Err(format!(
            "timeout: provider call exceeded {} seconds",
            timeout_seconds.clamp(1, 3_600)
        ));
    }
    if capture.code.unwrap_or(1) != 0 {
        return Err(format!(
            "provider call failed: {}",
            capture.stderr.lines().next().unwrap_or("curl failed")
        ));
    }
    let mut response = parse_curl_http_response(&capture.stdout);
    response.retry_after_ms = parse_retry_after_ms(&headers);
    Ok(response)
}

fn parse_curl_http_response(stdout: &str) -> ProviderHttpResponse {
    let marker = "\n__BENCHFORGE_HTTP_STATUS__:";
    let Some(index) = stdout.rfind(marker) else {
        return ProviderHttpResponse {
            body: stdout.to_string(),
            status: None,
            retry_after_ms: None,
            time_to_first_byte_ms: None,
            request_total_ms: None,
        };
    };
    let body = stdout[..index].to_string();
    let mut status = None;
    let mut time_to_first_byte_ms = None;
    let mut request_total_ms = None;
    for line in stdout[index + 1..].lines() {
        if let Some(value) = line.strip_prefix("__BENCHFORGE_HTTP_STATUS__:") {
            status = value
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|status| *status > 0);
        } else if let Some(value) = line.strip_prefix("__BENCHFORGE_TIME_STARTTRANSFER__:") {
            time_to_first_byte_ms = parse_curl_seconds_to_ms(value);
        } else if let Some(value) = line.strip_prefix("__BENCHFORGE_TIME_TOTAL__:") {
            request_total_ms = parse_curl_seconds_to_ms(value);
        }
    }
    ProviderHttpResponse {
        body,
        status,
        retry_after_ms: None,
        time_to_first_byte_ms,
        request_total_ms,
    }
}

fn parse_curl_seconds_to_ms(value: &str) -> Option<f64> {
    let seconds = value.trim().parse::<f64>().ok()?;
    seconds.is_finite().then_some(seconds * 1_000.0)
}

fn parse_retry_after_ms(headers: &str) -> Option<u64> {
    parse_retry_after_ms_at(headers, Utc::now())
}

fn parse_retry_after_ms_at(headers: &str, now: DateTime<Utc>) -> Option<u64> {
    headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("retry-after"))
        .filter_map(|(_, value)| parse_retry_after_value_ms_at(value.trim(), now))
        .last()
        .map(|value| value.min(MAX_PROVIDER_RETRY_AFTER_MS))
}

fn parse_retry_after_value_ms_at(value: &str, now: DateTime<Utc>) -> Option<u64> {
    if let Ok(seconds) = value.parse::<f64>() {
        if seconds.is_finite() && seconds >= 0.0 {
            return Some((seconds * 1_000.0).ceil() as u64);
        }
    }
    let retry_at = DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&Utc);
    let delay = retry_at.signed_duration_since(now).num_milliseconds();
    Some(delay.max(0) as u64)
}

fn apply_model_output(
    workspace: &Path,
    output: &str,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    ensure_not_cancelled(is_cancelled)?;
    let trimmed = output.trim();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(edits) = json.get("edits").and_then(|value| value.as_array()) {
            for edit in edits {
                let path = edit
                    .get("path")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| "edit missing path".to_string())?;
                let content = edit
                    .get("content")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| "edit missing content".to_string())?;
                let target = safety::safe_child_path(workspace, path)?;
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
                }
                fs::write(target, content).map_err(|err| err.to_string())?;
            }
            return Ok(());
        }
    }
    if trimmed.starts_with("diff --git") || trimmed.starts_with("--- ") {
        let patch_path = workspace.join(".benchforge-model.patch");
        fs::write(&patch_path, trimmed).map_err(|err| err.to_string())?;
        let capture = run_command_capture_checked(
            sandboxed_command_in(
                workspace,
                "git",
                &["apply", patch_path.to_string_lossy().as_ref()],
            )?,
            Duration::from_secs(30),
            is_cancelled,
        )?;
        let _ = fs::remove_file(patch_path);
        if capture.code.unwrap_or(1) == 0 {
            return Ok(());
        }
        return Err(format!(
            "model patch did not apply: {}",
            capture.stderr.lines().next().unwrap_or("git apply failed")
        ));
    }
    Err("model output did not match JSON edits or unified diff protocol".into())
}

fn list_workspace_files(workspace: &Path) -> String {
    let mut names = Vec::new();
    collect_workspace_files(workspace, workspace, &mut names);
    names.sort();
    names.join("\n")
}

fn collect_workspace_files(root: &Path, current: &Path, names: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == ".git"
            || entry.file_name() == "node_modules"
            || entry.file_name() == ".benchforge-venv"
        {
            continue;
        }
        if path.is_dir() {
            collect_workspace_files(root, &path, names);
        } else if let Ok(relative) = path.strip_prefix(root) {
            names.push(relative.to_string_lossy().to_string());
        }
    }
}

fn run_scoring_host(
    task: &TaskSpec,
    workspace: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(CommandCapture, ScoringCommandMetadata), String> {
    prepare_workspace_for_scoring(task, workspace, is_cancelled)?;
    let Some((command, args)) = task.scoring.command.split_first() else {
        return Err("scoring command is empty".into());
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let command = resolve_local_command(command, workspace);
    let metadata = capture_sandboxed_scoring_command_metadata(
        &task.scoring.command,
        &command,
        workspace,
        is_cancelled,
    )?;
    let capture = run_command_capture_checked(
        sandboxed_command_in(workspace, &command, &arg_refs)?,
        Duration::from_secs(task.timeout_seconds),
        is_cancelled,
    )?;
    Ok((capture, metadata))
}

fn capture_sandboxed_scoring_command_metadata(
    scoring_command: &[String],
    resolved_command: &str,
    workspace: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ScoringCommandMetadata, String> {
    capture_scoring_command_metadata(
        scoring_command,
        resolved_command,
        is_cancelled,
        |command, args| sandboxed_command_in(workspace, command, args),
    )
}

fn capture_command_version_metadata_at(
    scoring_command: &[String],
    resolved_command: &str,
    workdir: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ScoringCommandMetadata, String> {
    capture_scoring_command_metadata(
        scoring_command,
        resolved_command,
        is_cancelled,
        |command, args| Ok(command_at(workdir, command, args)),
    )
}

fn capture_cli_agent_command_metadata(
    adapter: &adapters::AdapterSpec,
    rendered_command: &[String],
    resolved_command: &str,
    workdir: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ScoringCommandMetadata, String> {
    let mut metadata =
        empty_scoring_command_metadata(rendered_command, Some(resolved_command.to_string()));
    let Some(probe) = cli_agent_version_probe(adapter, rendered_command, resolved_command) else {
        return Ok(metadata);
    };
    metadata.version_probe = Some(probe.clone());
    let Some((command, args)) = probe.split_first() else {
        return Ok(metadata);
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match run_command_capture_checked(
        command_at(workdir, command, &arg_refs),
        Duration::from_secs(15),
        is_cancelled,
    ) {
        Ok(capture) => apply_version_capture(&mut metadata, capture),
        Err(err) if err == "cancelled" => return Err(err),
        Err(err) => metadata.version_stderr = Some(err),
    }
    Ok(metadata)
}

fn cli_agent_version_probe(
    adapter: &adapters::AdapterSpec,
    rendered_command: &[String],
    resolved_command: &str,
) -> Option<Vec<String>> {
    if let Some(args) = adapter
        .validation
        .get("command_args")
        .and_then(|value| value.as_array())
    {
        let mut probe = Vec::with_capacity(args.len() + 1);
        probe.push(resolved_command.to_string());
        for arg in args {
            let arg = arg.as_str()?.trim();
            if !arg.is_empty() {
                probe.push(arg.to_string());
            }
        }
        return Some(probe);
    }
    scoring_version_probe(rendered_command, resolved_command)
}

fn capture_docker_scoring_command_metadata(
    image: &str,
    scoring_command: &[String],
    is_cancelled: &dyn Fn() -> bool,
) -> Result<ScoringCommandMetadata, String> {
    let Some(command) = scoring_command.first() else {
        return Ok(empty_scoring_command_metadata(scoring_command, None));
    };
    let mut metadata = empty_scoring_command_metadata(scoring_command, Some(command.clone()));
    let Some(probe) = scoring_version_probe(scoring_command, command) else {
        return Ok(metadata);
    };
    metadata.version_probe = Some(probe.clone());
    let container_name = format!("benchforge-version-{}", uuid::Uuid::new_v4().simple());
    let args = docker_scoring_version_run_args(&container_name, image, &probe);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match run_command_capture_checked_with_cleanup(
        command_at(&paths::resource_root(), "docker", &arg_refs),
        Duration::from_secs(15),
        is_cancelled,
        || force_remove_docker_container(&container_name),
    ) {
        Ok(capture) => apply_version_capture(&mut metadata, capture),
        Err(err) if err == "cancelled" => return Err(err),
        Err(err) => metadata.version_stderr = Some(err),
    }
    Ok(metadata)
}

fn capture_scoring_command_metadata(
    scoring_command: &[String],
    resolved_command: &str,
    is_cancelled: &dyn Fn() -> bool,
    command_builder: impl Fn(&str, &[&str]) -> Result<Command, String>,
) -> Result<ScoringCommandMetadata, String> {
    let mut metadata =
        empty_scoring_command_metadata(scoring_command, Some(resolved_command.to_string()));
    let Some(probe) = scoring_version_probe(scoring_command, resolved_command) else {
        return Ok(metadata);
    };
    metadata.version_probe = Some(probe.clone());
    let Some((command, args)) = probe.split_first() else {
        return Ok(metadata);
    };
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match command_builder(command, &arg_refs).and_then(|command| {
        run_command_capture_checked(command, Duration::from_secs(15), is_cancelled)
    }) {
        Ok(capture) => apply_version_capture(&mut metadata, capture),
        Err(err) if err == "cancelled" => return Err(err),
        Err(err) => metadata.version_stderr = Some(err),
    }
    Ok(metadata)
}

fn empty_scoring_command_metadata(
    scoring_command: &[String],
    resolved_command: Option<String>,
) -> ScoringCommandMetadata {
    ScoringCommandMetadata {
        command: scoring_command.to_vec(),
        resolved_command,
        version_probe: None,
        version_stdout: None,
        version_stderr: None,
        version_exit_code: None,
        version_timed_out: false,
    }
}

fn apply_version_capture(metadata: &mut ScoringCommandMetadata, capture: CommandCapture) {
    let secrets = collect_secret_values();
    metadata.version_stdout =
        clean_version_output(&safety::redact_secrets(&capture.stdout, &secrets));
    metadata.version_stderr =
        clean_version_output(&safety::redact_secrets(&capture.stderr, &secrets));
    metadata.version_exit_code = capture.code;
    metadata.version_timed_out = capture.timed_out;
}

fn clean_version_output(output: &str) -> Option<String> {
    let text = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        None
    } else {
        Some(safety::truncate_bytes(text, 8 * 1024))
    }
}

fn scoring_version_probe(
    scoring_command: &[String],
    resolved_command: &str,
) -> Option<Vec<String>> {
    let command = scoring_command.first()?.trim();
    if command.is_empty() {
        return None;
    }
    if is_python_command(command)
        && scoring_command
            .get(1)
            .is_some_and(|arg| arg.as_str() == "-m")
        && scoring_command
            .get(2)
            .is_some_and(|module| versionable_python_module(module))
    {
        return Some(vec![
            resolved_command.to_string(),
            "-m".into(),
            scoring_command[2].clone(),
            "--version".into(),
        ]);
    }
    Some(vec![resolved_command.to_string(), "--version".into()])
}

fn is_python_command(command: &str) -> bool {
    Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase().starts_with("python"))
        .unwrap_or(false)
}

fn versionable_python_module(module: &str) -> bool {
    matches!(module, "pip" | "pytest")
}

fn prepare_workspace_for_scoring(
    task: &TaskSpec,
    workspace: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    ensure_not_cancelled(is_cancelled)?;
    if task.language.as_deref() == Some("python") && workspace.join("requirements.txt").exists() {
        let venv_python = workspace
            .join(".benchforge-venv")
            .join("bin")
            .join("python");
        if !venv_python.exists() {
            let python = if adapters::command_exists("python3") {
                "python3"
            } else {
                "python"
            };
            let capture = run_command_capture_checked(
                sandboxed_command_in(workspace, python, &["-m", "venv", ".benchforge-venv"])?,
                Duration::from_secs(120),
                is_cancelled,
            )?;
            if capture.timed_out || capture.code.unwrap_or(1) != 0 {
                return Err(format!(
                    "python venv creation failed: {}",
                    capture.stderr.lines().next().unwrap_or("unknown error")
                ));
            }
        }
        let capture = run_command_capture_checked(
            sandboxed_command_in(
                workspace,
                venv_python.to_string_lossy().as_ref(),
                &["-m", "pip", "install", "-q", "-r", "requirements.txt"],
            )?,
            Duration::from_secs(180),
            is_cancelled,
        )?;
        if capture.timed_out || capture.code.unwrap_or(1) != 0 {
            return Err(format!(
                "pip install failed: {}",
                capture.stderr.lines().next().unwrap_or("unknown error")
            ));
        }
    }
    if task.language.as_deref() == Some("javascript")
        && workspace.join("package.json").exists()
        && !workspace.join("node_modules").exists()
    {
        let capture = run_command_capture_checked(
            sandboxed_command_in(workspace, "npm", &["install", "--silent"])?,
            Duration::from_secs(180),
            is_cancelled,
        )?;
        if capture.timed_out || capture.code.unwrap_or(1) != 0 {
            return Err(format!(
                "npm install failed: {}",
                capture.stderr.lines().next().unwrap_or("unknown error")
            ));
        }
    }
    Ok(())
}

fn resolve_local_command(command: &str, workspace: &Path) -> String {
    let venv_python = workspace
        .join(".benchforge-venv")
        .join("bin")
        .join("python");
    if command == "python" && venv_python.exists() {
        venv_python.to_string_lossy().to_string()
    } else if command == "python"
        && !adapters::command_exists("python")
        && adapters::command_exists("python3")
    {
        "python3".into()
    } else {
        command.into()
    }
}

fn run_scoring_docker(
    task: &TaskSpec,
    workspace: &Path,
    artifact_dir: &Path,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<
    (
        CommandCapture,
        DockerScoringImageMetadata,
        ScoringCommandMetadata,
    ),
    String,
> {
    let image = "benchforge-runner:local";
    let docker_dir = paths::resource_root().join("docker");
    let dockerfile_path = docker_dir.join("runner.Dockerfile");
    let dockerfile_sha256 = checksum_file(&dockerfile_path)?;
    let build_capture = run_command_capture_checked(
        command_at(
            &paths::resource_root(),
            "docker",
            &[
                "build",
                "-t",
                image,
                "-f",
                dockerfile_path.to_string_lossy().as_ref(),
                docker_dir.to_string_lossy().as_ref(),
            ],
        ),
        Duration::from_secs(300),
        is_cancelled,
    )?;
    if build_capture.timed_out || build_capture.code.unwrap_or(1) != 0 {
        let stderr = build_capture.stderr.trim();
        let stdout = build_capture.stdout.trim();
        let detail = match (stderr.is_empty(), stdout.is_empty()) {
            (false, false) => format!("{stderr}\n{stdout}"),
            (false, true) => stderr.to_string(),
            (true, false) => stdout.to_string(),
            (true, true) => "docker build exited without diagnostic output".to_string(),
        };
        return Err(format!("docker image build failed for {image}: {detail}"));
    }

    let image_metadata =
        inspect_docker_scoring_image(image, Some(dockerfile_sha256), is_cancelled)?;
    let command_metadata =
        capture_docker_scoring_command_metadata(image, &task.scoring.command, is_cancelled)?;

    let container_name = format!("benchforge-{}", uuid::Uuid::new_v4().simple());
    let args = docker_scoring_run_args(
        &container_name,
        workspace,
        artifact_dir,
        image,
        &task.scoring.command,
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let capture = run_command_capture_checked_with_cleanup(
        command_at(&paths::resource_root(), "docker", &arg_refs),
        Duration::from_secs(task.timeout_seconds),
        is_cancelled,
        || force_remove_docker_container(&container_name),
    )?;
    Ok((capture, image_metadata, command_metadata))
}

fn inspect_docker_scoring_image(
    image: &str,
    dockerfile_sha256: Option<String>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<DockerScoringImageMetadata, String> {
    let capture = run_command_capture_checked(
        command_at(
            &paths::resource_root(),
            "docker",
            &["image", "inspect", image],
        ),
        Duration::from_secs(30),
        is_cancelled,
    )?;
    if capture.timed_out || capture.code.unwrap_or(1) != 0 {
        return Err(format!(
            "docker image inspect failed for {}: {}",
            image,
            capture.stderr.trim()
        ));
    }
    docker_scoring_image_metadata_from_inspect(image, dockerfile_sha256, &capture.stdout)
}

fn docker_scoring_image_metadata_from_inspect(
    image: &str,
    dockerfile_sha256: Option<String>,
    inspect_json: &str,
) -> Result<DockerScoringImageMetadata, String> {
    let parsed: serde_json::Value = serde_json::from_str(inspect_json)
        .map_err(|err| format!("invalid docker inspect JSON: {err}"))?;
    let item = parsed
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| "docker image inspect returned no image records".to_string())?;
    let image_id = item
        .get("Id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let repo_digests = item
        .get("RepoDigests")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty() && *item != "<none>@<none>")
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let image_digest = repo_digests.first().cloned().or_else(|| image_id.clone());
    if image_digest.is_none() {
        return Err("docker image inspect did not include an image ID or repo digest".into());
    }
    Ok(DockerScoringImageMetadata {
        image: image.to_string(),
        image_id,
        image_digest,
        repo_digests,
        dockerfile_sha256,
    })
}

fn docker_scoring_run_args(
    container_name: &str,
    workspace: &Path,
    artifact_dir: &Path,
    image: &str,
    scoring_command: &[String],
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        container_name.to_string(),
        "--network".to_string(),
        "none".to_string(),
    ];
    append_docker_scoring_sandbox_args(&mut args);
    args.extend([
        "-v".to_string(),
        format!("{}:/workspace", workspace.to_string_lossy()),
        "-v".to_string(),
        format!("{}:/artifacts", artifact_dir.to_string_lossy()),
        "-w".to_string(),
        "/workspace".to_string(),
        image.to_string(),
    ]);
    args.extend(scoring_command.iter().cloned());
    args
}

fn docker_scoring_version_run_args(
    container_name: &str,
    image: &str,
    version_probe: &[String],
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        container_name.to_string(),
        "--network".to_string(),
        "none".to_string(),
    ];
    append_docker_scoring_sandbox_args(&mut args);
    args.push(image.to_string());
    args.extend(version_probe.iter().cloned());
    args
}

fn append_docker_scoring_sandbox_args(args: &mut Vec<String>) {
    args.extend([
        "--cpus".to_string(),
        DOCKER_SCORING_CPUS.to_string(),
        "--memory".to_string(),
        DOCKER_SCORING_MEMORY.to_string(),
        "--pids-limit".to_string(),
        DOCKER_SCORING_PIDS_LIMIT.to_string(),
        "--cap-drop".to_string(),
        DOCKER_SCORING_CAP_DROP.to_string(),
        "--security-opt".to_string(),
        DOCKER_SCORING_SECURITY_OPT.to_string(),
    ]);
}

fn docker_scoring_resource_limits() -> serde_json::Value {
    serde_json::json!({
        "cpus": DOCKER_SCORING_CPUS,
        "memory": DOCKER_SCORING_MEMORY,
        "pids_limit": DOCKER_SCORING_PIDS_LIMIT,
        "cap_drop": [DOCKER_SCORING_CAP_DROP],
        "security_opt": [DOCKER_SCORING_SECURITY_OPT]
    })
}

fn repo_code_commands_observed_count(target: &store::TargetRecord, task: &TaskSpec) -> u64 {
    let mut count = 0;
    if target.kind == "cli_agent" {
        count += 1;
    }
    if !task.scoring.command.is_empty() {
        count += 1;
    }
    count
}

fn force_remove_docker_container(container_name: &str) {
    let _ = adapters::command_with_gui_path("docker")
        .args(["rm", "-f", container_name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn run_git_diff(workspace: &Path, is_cancelled: &dyn Fn() -> bool) -> Result<String, String> {
    git_intent_to_add_untracked(workspace, is_cancelled)?;
    let mut args = vec!["diff".to_string(), "HEAD".to_string()];
    args.extend(git_pathspec_args());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let capture = run_git_command(workspace, &arg_refs, is_cancelled)?;
    Ok(capture.stdout)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiffChangeStats {
    files_changed: u64,
    lines_added: u64,
    lines_deleted: u64,
}

fn diff_change_stats(diff: &str) -> DiffChangeStats {
    let mut stats = DiffChangeStats {
        files_changed: 0,
        lines_added: 0,
        lines_deleted: 0,
    };
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            stats.files_changed += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            stats.lines_added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            stats.lines_deleted += 1;
        }
    }
    stats
}

fn command_in(workdir: &Path, command: &str, args: &[&str]) -> Command {
    command_at(workdir, command, args)
}

fn command_at(workdir: &Path, command: &str, args: &[&str]) -> Command {
    let mut cmd = adapters::command_with_gui_path(command);
    cmd.current_dir(workdir).args(args);
    cmd
}

fn sandboxed_command_in(workspace: &Path, command: &str, args: &[&str]) -> Result<Command, String> {
    let mut cmd = Command::new(command);
    cmd.current_dir(workspace).args(args);
    apply_sandboxed_scoring_env(&mut cmd, workspace)?;
    Ok(cmd)
}

fn apply_sandboxed_scoring_env(cmd: &mut Command, workspace: &Path) -> Result<(), String> {
    let env = sandboxed_scoring_env(workspace)?;
    cmd.env_clear();
    for (key, value) in env {
        cmd.env(key, value);
    }
    Ok(())
}

fn sandboxed_scoring_env(workspace: &Path) -> Result<HashMap<String, String>, String> {
    let home = workspace.join(SANDBOX_HOME_DIR);
    let tmp = workspace.join(SANDBOX_TMP_DIR);
    let npm_cache = workspace.join(SANDBOX_NPM_CACHE_DIR);
    fs::create_dir_all(&home).map_err(|err| err.to_string())?;
    fs::create_dir_all(&tmp).map_err(|err| err.to_string())?;
    fs::create_dir_all(&npm_cache).map_err(|err| err.to_string())?;

    let mut env = HashMap::new();
    env.insert("PATH".into(), adapters::gui_path());
    env.insert("HOME".into(), home.to_string_lossy().to_string());
    env.insert("TMPDIR".into(), tmp.to_string_lossy().to_string());
    env.insert("TMP".into(), tmp.to_string_lossy().to_string());
    env.insert("TEMP".into(), tmp.to_string_lossy().to_string());
    env.insert("CI".into(), "1".into());
    env.insert("NO_COLOR".into(), "1".into());
    env.insert("PYTHONNOUSERSITE".into(), "1".into());
    env.insert("PIP_DISABLE_PIP_VERSION_CHECK".into(), "1".into());
    env.insert("PIP_NO_INPUT".into(), "1".into());
    env.insert(
        "NPM_CONFIG_CACHE".into(),
        npm_cache.to_string_lossy().to_string(),
    );
    env.insert("NPM_CONFIG_AUDIT".into(), "false".into());
    env.insert("NPM_CONFIG_FUND".into(), "false".into());
    env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());

    #[cfg(windows)]
    {
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            env.insert(
                "SystemRoot".into(),
                system_root.to_string_lossy().to_string(),
            );
        }
        if let Some(comspec) = std::env::var_os("COMSPEC") {
            env.insert("COMSPEC".into(), comspec.to_string_lossy().to_string());
        }
    }

    Ok(env)
}

fn sandbox_environment_label(docker: bool, task: &TaskSpec) -> &'static str {
    if uses_docker_scoring(docker, task) {
        "docker-network-none"
    } else {
        "sanitized-host"
    }
}

fn sandbox_level(docker: bool, task: &TaskSpec) -> u8 {
    if uses_docker_scoring(docker, task) {
        2
    } else {
        1
    }
}

fn permission_mode_label(docker: bool, task: &TaskSpec) -> &'static str {
    if uses_docker_scoring(docker, task) {
        "patch-basic-docker-scoring"
    } else {
        "patch-basic-host-scoring"
    }
}

fn host_reproducibility() -> serde_json::Value {
    let logical_cores = std::thread::available_parallelism()
        .ok()
        .map(|count| count.get());
    let mut hardware = serde_json::Map::new();
    if let Some(value) = logical_cores {
        hardware.insert("logical_cores".into(), serde_json::json!(value));
    }
    if let Some(value) = total_memory_bytes() {
        hardware.insert("memory_bytes".into(), serde_json::json!(value));
    }
    if let Some(value) = cpu_brand() {
        hardware.insert("cpu_brand".into(), serde_json::json!(value));
    }
    if let Some(value) = machine_model() {
        hardware.insert("machine_model".into(), serde_json::json!(value));
    }

    serde_json::json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "kernel": kernel_release(),
        "hardware": hardware,
    })
}

fn kernel_release() -> Option<String> {
    command_first_line("uname", &["-r"])
}

#[cfg(target_os = "macos")]
fn total_memory_bytes() -> Option<u64> {
    command_first_line("sysctl", &["-n", "hw.memsize"])?
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
fn cpu_brand() -> Option<String> {
    command_first_line("sysctl", &["-n", "machdep.cpu.brand_string"])
}

#[cfg(target_os = "macos")]
fn machine_model() -> Option<String> {
    command_first_line("sysctl", &["-n", "hw.model"])
}

#[cfg(target_os = "linux")]
fn total_memory_bytes() -> Option<u64> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let kb = content.lines().find_map(|line| {
        let rest = line.strip_prefix("MemTotal:")?.trim();
        rest.split_whitespace().next()?.parse::<u64>().ok()
    })?;
    kb.checked_mul(1024)
}

#[cfg(target_os = "linux")]
fn cpu_brand() -> Option<String> {
    let content = fs::read_to_string("/proc/cpuinfo").ok()?;
    content.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if !matches!(key.trim(), "model name" | "Hardware" | "Processor") {
            return None;
        }
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

#[cfg(target_os = "linux")]
fn machine_model() -> Option<String> {
    fs::read_to_string("/sys/devices/virtual/dmi/id/product_name")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn total_memory_bytes() -> Option<u64> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cpu_brand() -> Option<String> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn machine_model() -> Option<String> {
    None
}

fn command_first_line(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

enum StreamStdoutEvent {
    Line(String, Duration),
    Done,
}

fn run_streaming_curl_capture(
    mut command: Command,
    timeout: Duration,
    format: StreamFormat,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<StreamCommandCapture, String> {
    ensure_not_cancelled(is_cancelled)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let start = Instant::now();
    let mut child = command.spawn().map_err(|err| err.to_string())?;
    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "stream stdout was not captured".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "stream stderr was not captured".to_string())?;

    let (tx, rx) = mpsc::channel();
    let stdout_start = start;
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = tx.send(StreamStdoutEvent::Line(
                        line.clone(),
                        stdout_start.elapsed(),
                    ));
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(StreamStdoutEvent::Done);
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut stderr = String::new();
        let _ = reader.read_to_string(&mut stderr);
        stderr
    });

    let mut state = StreamParseState::new(format);
    let mut status = None;
    let mut stdout_done = false;
    let mut timed_out = false;

    loop {
        if is_cancelled() {
            kill_process_group(pid);
            let _ = child.wait();
            return Err("cancelled".into());
        }
        if !timed_out && start.elapsed() >= timeout {
            timed_out = true;
            kill_process_group(pid);
            status = Some(child.wait().map_err(|err| err.to_string())?);
        }
        while let Ok(event) = rx.try_recv() {
            match event {
                StreamStdoutEvent::Line(line, elapsed) => {
                    state.handle_stdout_line(&line, elapsed.as_secs_f64() * 1_000.0);
                }
                StreamStdoutEvent::Done => stdout_done = true,
            }
        }
        if status.is_none() {
            if let Some(exit) = child.try_wait().map_err(|err| err.to_string())? {
                status = Some(exit);
            }
        }
        if status.is_some() && stdout_done {
            break;
        }
        if timed_out && stdout_done {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(StreamStdoutEvent::Line(line, elapsed)) => {
                state.handle_stdout_line(&line, elapsed.as_secs_f64() * 1_000.0);
            }
            Ok(StreamStdoutEvent::Done) => stdout_done = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => stdout_done = true,
        }
    }

    let status = match status {
        Some(status) => status,
        None => child.wait().map_err(|err| err.to_string())?,
    };
    while let Ok(event) = rx.try_recv() {
        match event {
            StreamStdoutEvent::Line(line, elapsed) => {
                state.handle_stdout_line(&line, elapsed.as_secs_f64() * 1_000.0);
            }
            StreamStdoutEvent::Done => {}
        }
    }
    let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
    let (response, stream_error) = state.finish(elapsed_ms);
    let stderr = stderr_handle.join().unwrap_or_default();
    Ok(StreamCommandCapture {
        response,
        stream_error,
        stderr: safety::truncate_bytes(stderr, MAX_OUTPUT_BYTES),
        code: status.code(),
        timed_out,
    })
}

fn run_command_capture(command: Command, timeout: Duration) -> Result<CommandCapture, String> {
    run_command_capture_checked(command, timeout, &|| false)
}

fn run_command_capture_checked(
    command: Command,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<CommandCapture, String> {
    run_command_capture_checked_with_cleanup(command, timeout, is_cancelled, || {})
}

fn run_command_capture_checked_with_cleanup(
    mut command: Command,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
    forced_cleanup: impl Fn(),
) -> Result<CommandCapture, String> {
    ensure_not_cancelled(is_cancelled)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let start = Instant::now();
    let child = command.spawn().map_err(|err| err.to_string())?;
    let pid = child.id();
    let mut peak_rss_mb = None;
    update_peak_rss_mb(&mut peak_rss_mb, pid);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let output = child.wait_with_output();
        let _ = tx.send(output);
    });

    let (output, timed_out) = loop {
        update_peak_rss_mb(&mut peak_rss_mb, pid);
        if is_cancelled() {
            kill_process_group(pid);
            forced_cleanup();
            let _ = rx.recv_timeout(Duration::from_secs(5));
            return Err("cancelled".into());
        }
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            kill_process_group(pid);
            forced_cleanup();
            let output = rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "process did not exit after timeout".to_string())?;
            break (output.map_err(|err| err.to_string())?, true);
        }
        let wait_for = (timeout - elapsed).min(Duration::from_millis(100));
        match rx.recv_timeout(wait_for) {
            Ok(output) => break (output.map_err(|err| err.to_string())?, false),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(err) => return Err(err.to_string()),
        }
    };
    update_peak_rss_mb(&mut peak_rss_mb, pid);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(CommandCapture {
        stdout: safety::truncate_bytes(stdout, MAX_OUTPUT_BYTES),
        stderr: safety::truncate_bytes(stderr, MAX_OUTPUT_BYTES),
        code: output.status.code(),
        timed_out,
        wall_time_ms: start.elapsed().as_millis() as u64,
        peak_rss_mb,
    })
}

fn update_peak_rss_mb(current: &mut Option<f64>, pid: u32) {
    if let Some(value) = process_tree_rss_mb(pid) {
        if current.map_or(true, |previous| value > previous) {
            *current = Some(value);
        }
    }
}

#[cfg(unix)]
fn process_tree_rss_mb(pid: u32) -> Option<f64> {
    let mut pids = vec![pid];
    if let Ok(output) = Command::new("pgrep")
        .args(["-g", &pid.to_string()])
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Ok(value) = line.trim().parse::<u32>() {
                    pids.push(value);
                }
            }
        }
    }
    pids.sort_unstable();
    pids.dedup();
    let pid_list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid_list])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rss_kb = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .sum::<u64>();
    (rss_kb > 0).then_some(rss_kb as f64 / 1024.0)
}

#[cfg(not(unix))]
fn process_tree_rss_mb(_pid: u32) -> Option<f64> {
    None
}

fn ensure_not_cancelled(is_cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if is_cancelled() {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}

fn kill_process_group(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{}", pid))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(500));
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{}", pid))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}

fn write_artifact(dir: &Path, relative: &str, content: &str) -> Result<PathBuf, String> {
    let path = safety::safe_child_path(dir, relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut file = fs::File::create(&path).map_err(|err| err.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|err| err.to_string())?;
    Ok(path)
}

fn write_cli_agent_evidence_artifacts(
    dir: &Path,
    run: &CliAgentRun,
    task_prompt: &str,
    secret_values: &[String],
) -> Result<CliAgentEvidenceArtifacts, String> {
    let stdout = safety::redact_secrets(&run.capture.stdout, secret_values);
    let stderr = safety::redact_secrets(&run.capture.stderr, secret_values);
    let stdout_path = write_artifact(dir, "cli-stdout.txt", &stdout)?;
    let stderr_path = write_artifact(dir, "cli-stderr.txt", &stderr)?;
    let command_metadata =
        redacted_cli_agent_command_metadata(&run.command_metadata, task_prompt, secret_values);
    let env = redacted_cli_agent_env(&run.env, task_prompt, secret_values);
    let evidence = serde_json::json!({
        "command_metadata": command_metadata,
        "working_dir": run.working_dir.to_string_lossy(),
        "env": env,
        "exit_code": run.capture.code,
        "timed_out": run.capture.timed_out,
        "wall_time_ms": run.capture.wall_time_ms,
        "peak_rss_mb": run.capture.peak_rss_mb,
        "stdout_bytes": stdout.len(),
        "stderr_bytes": stderr.len(),
        "stdout_sha256": checksum_text(&stdout),
        "stderr_sha256": checksum_text(&stderr),
        "transcript_files": {
            "stdout": stdout_path.to_string_lossy(),
            "stderr": stderr_path.to_string_lossy()
        }
    });
    let command_path = write_artifact(
        dir,
        "cli-agent-command.json",
        &serde_json::to_string_pretty(&evidence).map_err(|err| err.to_string())?,
    )?;
    Ok(CliAgentEvidenceArtifacts {
        stdout_path,
        stderr_path,
        command_path,
        evidence,
        stdout_bytes: stdout.len(),
        stderr_bytes: stderr.len(),
    })
}

fn redacted_cli_agent_command_metadata(
    metadata: &ScoringCommandMetadata,
    task_prompt: &str,
    secret_values: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "command": redact_cli_agent_command_vec(&metadata.command, task_prompt, secret_values),
        "resolved_command": metadata.resolved_command.as_ref().map(|value| {
            redact_cli_agent_command_part(value, task_prompt, secret_values)
        }),
        "version_probe": metadata.version_probe.as_ref().map(|value| {
            redact_cli_agent_command_vec(value, task_prompt, secret_values)
        }),
        "version_stdout": metadata.version_stdout.as_ref().map(|value| {
            safety::redact_secrets(value, secret_values)
        }),
        "version_stderr": metadata.version_stderr.as_ref().map(|value| {
            safety::redact_secrets(value, secret_values)
        }),
        "version_exit_code": metadata.version_exit_code,
        "version_timed_out": metadata.version_timed_out
    })
}

fn redact_cli_agent_command_vec(
    values: &[String],
    task_prompt: &str,
    secret_values: &[String],
) -> Vec<String> {
    values
        .iter()
        .map(|value| redact_cli_agent_command_part(value, task_prompt, secret_values))
        .collect()
}

fn redacted_cli_agent_env(
    env: &BTreeMap<String, String>,
    task_prompt: &str,
    secret_values: &[String],
) -> serde_json::Value {
    let mut redacted = serde_json::Map::new();
    for (key, value) in env {
        redacted.insert(
            key.clone(),
            serde_json::json!(redact_cli_agent_command_part(
                value,
                task_prompt,
                secret_values
            )),
        );
    }
    serde_json::Value::Object(redacted)
}

fn redact_cli_agent_command_part(
    value: &str,
    task_prompt: &str,
    secret_values: &[String],
) -> String {
    let prompt_elided = if task_prompt.is_empty() {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(value.replace(task_prompt, "<task_prompt>"))
    };
    safety::redact_secrets(prompt_elided.as_ref(), secret_values)
}

fn checksum_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| format!("{}: {}", path.display(), err))?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{:x}", digest))
}

fn checksum_text(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!("{:x}", digest)
}

fn optional_prompt(prompt: &str) -> Option<&str> {
    if prompt.is_empty() {
        None
    } else {
        Some(prompt)
    }
}

fn prompt_reproducibility(
    task_prompt: Option<&str>,
    system_prompt: Option<&str>,
    user_prompt: Option<&str>,
    tool_prompt: Option<&str>,
) -> serde_json::Value {
    let mut prompts = serde_json::Map::new();
    prompts.insert("hash_algorithm".into(), serde_json::json!("sha256"));
    insert_prompt_hash(&mut prompts, "task_prompt", task_prompt);
    insert_prompt_hash(&mut prompts, "system_prompt", system_prompt);
    insert_prompt_hash(&mut prompts, "user_prompt", user_prompt);
    insert_prompt_hash(&mut prompts, "tool_prompt", tool_prompt);
    serde_json::Value::Object(prompts)
}

fn insert_prompt_hash(
    prompts: &mut serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    prompt: Option<&str>,
) {
    if let Some(prompt) = prompt {
        prompts.insert(
            format!("{prefix}_sha256"),
            serde_json::json!(checksum_text(prompt)),
        );
        prompts.insert(
            format!("{prefix}_chars"),
            serde_json::json!(prompt.chars().count()),
        );
    }
}

fn collect_secret_values() -> Vec<String> {
    [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "MISTRAL_API_KEY",
        "OPENROUTER_API_KEY",
        "AZURE_OPENAI_API_KEY",
        "HF_TOKEN",
        "HUGGINGFACE_TOKEN",
        "GITHUB_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
    ]
    .iter()
    .filter_map(|name| std::env::var(name).ok())
    .filter(|value| !value.is_empty())
    .collect()
}

fn worker_command() -> String {
    let venv_worker = paths::worker_venv_launcher();
    if venv_worker.exists() {
        venv_worker.to_string_lossy().to_string()
    } else if paths::bundled_worker_launcher().exists() {
        paths::bundled_worker_launcher()
            .to_string_lossy()
            .to_string()
    } else {
        "benchforge-worker".into()
    }
}

fn parse_worker_final_event(jsonl: &str) -> Option<serde_json::Value> {
    jsonl.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
        let is_finished = value
            .get("type")
            .and_then(|kind| kind.as_str())
            .is_some_and(|kind| kind == "run_finished")
            || value.get("status").is_some();
        is_finished.then_some(value)
    })
}

fn worker_diagnostics(event: &serde_json::Value) -> Vec<String> {
    event
        .pointer("/safety/diagnostics")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("message").and_then(|value| value.as_str()))
        .map(str::to_string)
        .collect()
}

fn worker_declared_artifacts(event: &serde_json::Value, run_dir: &Path) -> Vec<(String, PathBuf)> {
    let Some(items) = event.get("artifacts").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    let Ok(run_root) = run_dir.canonicalize() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let path = item.get("path").and_then(|value| value.as_str())?;
            let candidate = PathBuf::from(path);
            let candidate = if candidate.is_absolute() {
                candidate
            } else {
                run_dir.join(candidate)
            };
            let canonical = candidate.canonicalize().ok()?;
            if !canonical.starts_with(&run_root) || !canonical.is_file() {
                return None;
            }
            let kind = item
                .get("kind")
                .and_then(|value| value.as_str())
                .map(normalize_worker_artifact_kind)
                .filter(|kind| !kind.is_empty())
                .unwrap_or_else(|| "worker_artifact".into());
            Some((kind, canonical))
        })
        .collect()
}

fn normalize_worker_artifact_kind(kind: &str) -> String {
    kind.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn worker_run_error_code(
    status: &str,
    event: &serde_json::Value,
    finding_count: Option<f64>,
) -> Option<String> {
    if status == "passed" {
        return None;
    }
    if let Some(code) = event.get("error_code").and_then(|value| value.as_str()) {
        if !code.is_empty() {
            return Some(code.to_string());
        }
    }
    match status {
        "failed" if finding_count.unwrap_or(0.0) > 0.0 => Some("security_findings".into()),
        "failed" => Some("benchmark_failed".into()),
        "timeout" => Some("timeout".into()),
        _ => Some("worker_failed".into()),
    }
}

fn worker_run_error_message(
    status: &str,
    event: &serde_json::Value,
    finding_count: Option<f64>,
    error_code: Option<&str>,
) -> Option<String> {
    if status == "passed" {
        return None;
    }
    if let Some(message) = event.get("error_message").and_then(|value| value.as_str()) {
        if !message.is_empty() {
            return Some(message.to_string());
        }
    }
    if error_code == Some("security_findings") {
        return Some(format!(
            "{} security finding(s) detected",
            finding_count.unwrap_or(0.0) as u64
        ));
    }
    Some(status.to_string())
}

fn parse_test_summary(stdout: &str, stderr: &str, parser: Option<&str>) -> serde_json::Value {
    let combined = format!("{}\n{}", stdout, stderr);
    match parser {
        Some("pytest") => {
            serde_json::json!({"framework": "pytest", "passed": combined.contains(" passed"), "raw": combined.lines().last().unwrap_or("")})
        }
        Some("jest") => {
            serde_json::json!({"framework": "jest", "passed": combined.contains("PASS") || combined.contains("Tests:") && combined.contains("passed"), "raw": combined.lines().rev().take(3).collect::<Vec<_>>().join("\n")})
        }
        Some("unittest") => {
            serde_json::json!({"framework": "unittest", "passed": combined.contains("OK"), "raw": combined.lines().rev().take(3).collect::<Vec<_>>().join("\n")})
        }
        Some("node-test") => {
            serde_json::json!({"framework": "node:test", "passed": combined.contains("# pass") && combined.contains("# fail 0"), "raw": combined.lines().rev().take(6).collect::<Vec<_>>().join("\n")})
        }
        other => {
            serde_json::json!({"framework": other, "raw": combined.lines().last().unwrap_or("")})
        }
    }
}

const PYTHON_RATE_LIMIT_FIX: &str = r#"from dataclasses import dataclass

USERS = {"alice": "correct-horse-battery-staple"}
FAILED_ATTEMPTS: dict[str, int] = {}
MAX_FAILURES = 3


@dataclass
class Response:
    status_code: int
    body: dict


def login(username: str, password: str, ip_address: str) -> Response:
    """Simple login handler used by the benchmark fixture."""
    if FAILED_ATTEMPTS.get(ip_address, 0) >= MAX_FAILURES:
        return Response(429, {"ok": False, "error": "rate limited"})
    if USERS.get(username) == password:
        FAILED_ATTEMPTS.pop(ip_address, None)
        return Response(200, {"ok": True})
    FAILED_ATTEMPTS[ip_address] = FAILED_ATTEMPTS.get(ip_address, 0) + 1
    return Response(401, {"ok": False, "error": "invalid credentials"})
"#;

const JS_SANITIZE_FIX: &str = r#"function sanitizeFilename(name) {
  const value = String(name).trim();
  if (!value) {
    throw new Error('filename is required');
  }
  return value.replace(/[\\/\x00-\x1f\x7f]/g, '_');
}

module.exports = { sanitizeFilename };
"#;

const PYTHON_CONFIG_MERGE_FIX: &str = r#"from copy import deepcopy


def merge_config(base: dict, override: dict) -> dict:
    """Recursively merge override values into a new configuration dictionary."""
    result = deepcopy(base)
    for key, value in override.items():
        current = result.get(key)
        if isinstance(current, dict) and isinstance(value, dict):
            result[key] = merge_config(current, value)
        else:
            result[key] = deepcopy(value)
    return result
"#;

const JS_RETRY_DELAY_FIX: &str = r#"function retryDelay(attempt) {
  if (!Number.isInteger(attempt) || attempt < 0) {
    throw new TypeError('attempt must be a non-negative integer');
  }
  return Math.min(100 * 2 ** attempt, 1600);
}

module.exports = { retryDelay };
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn cloud_contract_test_attempts(
    ) -> Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>> {
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()))
    }

    fn test_task(id: &str, task_type: &str, language: Option<&str>) -> TaskSpec {
        TaskSpec {
            id: id.into(),
            name: id.into(),
            task_type: task_type.into(),
            version: Some("0.1.0".into()),
            language: language.map(str::to_string),
            fixture: None,
            prompt: String::new(),
            timeout_seconds: 60,
            max_turns: None,
            weight: 1.0,
            scoring: ScoringSpec {
                command: vec![],
                parse: None,
                expect_exact: None,
                expect_contains: vec![],
                expect_regex: vec![],
                expect_not_contains: vec![],
                expect_json: false,
                json_field_equals: HashMap::new(),
                json_field_contains: HashMap::new(),
                json_field_object_keys_exact: HashMap::new(),
                json_field_array_exact: HashMap::new(),
                json_field_array_exact_ordered: HashMap::new(),
                json_field_number_close: HashMap::new(),
                json_field_number_bounds: HashMap::new(),
            },
            source_path: PathBuf::new(),
        }
    }

    fn test_pack(id: &str) -> BenchmarkPackSpec {
        BenchmarkPackSpec {
            id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            description: None,
            tags: vec![],
            estimated_runtime: None,
            requires_sandbox: false,
            calibration: None,
            tasks: vec![],
            pack_dir: PathBuf::new(),
            pack_path: PathBuf::new(),
            source: "test".into(),
        }
    }

    fn write_test_prompt_pack(root: &Path, pack_id: &str, task_id: &str, expected: &str) {
        let pack_dir = root.join(pack_id);
        let task_dir = pack_dir.join("tasks");
        fs::create_dir_all(&task_dir).expect("task dir should create");
        fs::write(
            pack_dir.join("pack.yaml"),
            format!(
                r#"
id: {pack_id}
name: Test {pack_id}
version: 0.1.0
description: User-owned prompt pack for private workload checks.
tags:
  - private
  - prompt
estimated_runtime: minutes
tasks:
  - tasks/{task_id}.yaml
"#
            ),
        )
        .expect("pack should write");
        fs::write(
            task_dir.join(format!("{task_id}.yaml")),
            format!(
                r#"
id: {task_id}
name: Test {task_id}
type: prompt
version: 0.1.0
prompt: Return exactly {expected}.
scoring:
  expect_exact: "{expected}"
"#
            ),
        )
        .expect("task should write");
    }

    fn test_target(id: &str, kind: &str) -> store::TargetRecord {
        store::TargetRecord {
            id: id.into(),
            name: id.into(),
            kind: kind.into(),
            adapter_id: kind.into(),
            config_json: "{}".into(),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        }
    }

    fn test_model_target(
        id: &str,
        adapter_id: &str,
        config: serde_json::Value,
    ) -> store::TargetRecord {
        store::TargetRecord {
            id: id.into(),
            name: id.into(),
            kind: "direct_model".into(),
            adapter_id: adapter_id.into(),
            config_json: serde_json::to_string(&config).expect("config should serialize"),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        }
    }

    fn test_harness_target(id: &str, config: serde_json::Value) -> store::TargetRecord {
        store::TargetRecord {
            id: id.into(),
            name: id.into(),
            kind: "benchmark_harness".into(),
            adapter_id: "benchforge-worker".into(),
            config_json: serde_json::to_string(&config).expect("config should serialize"),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        }
    }

    fn test_worker_harness_task(id: &str, worker_kind: &str) -> TaskSpec {
        let mut task = test_task(id, "benchmark_harness", Some("python"));
        task.scoring.command = vec![
            "benchforge-worker".into(),
            "run".into(),
            "--kind".into(),
            worker_kind.into(),
        ];
        task
    }

    fn assert_required_v1_metrics_are_explicit(metrics: &serde_json::Value) {
        let metrics = metrics
            .as_object()
            .expect("metrics should be a JSON object");
        for key in REQUIRED_V1_METRIC_KEYS {
            assert!(metrics.contains_key(*key), "missing required metric {key}");
        }
    }

    #[test]
    fn scoring_version_probe_prefers_supported_python_modules() {
        assert_eq!(
            scoring_version_probe(
                &["python".into(), "-m".into(), "pytest".into(), "-q".into()],
                "/tmp/workspace/.benchforge-venv/bin/python"
            ),
            Some(vec![
                "/tmp/workspace/.benchforge-venv/bin/python".into(),
                "-m".into(),
                "pytest".into(),
                "--version".into()
            ])
        );
        assert_eq!(
            scoring_version_probe(
                &[
                    "python3".into(),
                    "-m".into(),
                    "unittest".into(),
                    "-q".into()
                ],
                "python3"
            ),
            Some(vec!["python3".into(), "--version".into()])
        );
        assert_eq!(
            scoring_version_probe(&["npm".into(), "test".into()], "npm"),
            Some(vec!["npm".into(), "--version".into()])
        );
    }

    #[test]
    fn target_compatibility_accepts_prompt_model_targets() {
        let pack = test_pack("llm-basics");
        let tasks = vec![test_task("prompt", "prompt", None)];
        let targets = vec![
            test_target("mock-agent", "mock"),
            test_target("local-model", "direct_model"),
            test_target("hf-model", "harnessed_model"),
        ];
        let target_ids = vec![
            "mock-agent".to_string(),
            "local-model".to_string(),
            "hf-model".to_string(),
        ];

        assert!(validate_target_compatibility(&pack, &tasks, &targets, &target_ids).is_ok());
    }

    #[test]
    fn target_compatibility_rejects_cli_for_prompt_pack() {
        let pack = test_pack("llm-basics");
        let tasks = vec![test_task("prompt", "prompt", None)];
        let targets = vec![test_target("codex-cli", "cli_agent")];
        let target_ids = vec!["codex-cli".to_string()];
        let err = validate_target_compatibility(&pack, &tasks, &targets, &target_ids)
            .expect_err("CLI targets cannot run prompt packs");

        assert!(err.contains("incompatible_target"));
        assert!(err.contains("direct_model"));
        assert!(err.contains("harnessed_model"));
    }

    #[test]
    fn target_compatibility_accepts_repo_code_targets() {
        let pack = test_pack("quick-smoke");
        let tasks = vec![test_task("repo", "repo_patch", Some("python"))];
        let targets = vec![
            test_target("codex-cli", "cli_agent"),
            test_target("local-model", "direct_model"),
            test_target("hf-model", "harnessed_model"),
            test_target("mock-agent", "mock"),
        ];
        let target_ids = targets
            .iter()
            .map(|target| target.id.clone())
            .collect::<Vec<_>>();

        assert!(validate_target_compatibility(&pack, &tasks, &targets, &target_ids).is_ok());
    }

    #[test]
    fn target_compatibility_requires_targets_to_support_every_task() {
        let pack = test_pack("mixed-pack");
        let tasks = vec![
            test_task("prompt", "prompt", None),
            test_task("repo", "repo_patch", Some("python")),
        ];
        let cli_target = vec![test_target("codex-cli", "cli_agent")];
        let cli_ids = vec!["codex-cli".to_string()];
        let model_target = vec![test_target("local-model", "direct_model")];
        let model_ids = vec!["local-model".to_string()];

        let err = validate_target_compatibility(&pack, &tasks, &cli_target, &cli_ids)
            .expect_err("CLI targets cannot run mixed prompt/code packs");
        assert!(err.contains("incompatible_target"));
        assert!(validate_target_compatibility(&pack, &tasks, &model_target, &model_ids).is_ok());
    }

    #[test]
    fn target_compatibility_accepts_only_benchmark_harness_for_harness_tasks() {
        let pack = test_pack("evalplus");
        let tasks = vec![test_task("evalplus", "benchmark_harness", Some("python"))];
        let harness = vec![test_target("worker", "benchmark_harness")];
        let harness_ids = vec!["worker".to_string()];
        let model = vec![test_target("local-model", "direct_model")];
        let model_ids = vec!["local-model".to_string()];

        assert!(validate_target_compatibility(&pack, &tasks, &harness, &harness_ids).is_ok());
        let err = validate_target_compatibility(&pack, &tasks, &model, &model_ids)
            .expect_err("model targets cannot run worker harness packs");
        assert!(err.contains("benchmark_harness"));
    }

    #[test]
    fn target_compatibility_reports_missing_target_ids() {
        let pack = test_pack("llm-basics");
        let tasks = vec![test_task("prompt", "prompt", None)];
        let targets = vec![test_target("mock-agent", "mock")];
        let target_ids = vec!["missing".to_string()];
        let err = validate_target_compatibility(&pack, &tasks, &targets, &target_ids)
            .expect_err("missing targets should fail before queueing");

        assert_eq!(err, "target_not_found: missing");
    }

    #[test]
    fn target_compatibility_rejects_disabled_targets() {
        let pack = test_pack("llm-basics");
        let tasks = vec![test_task("prompt", "prompt", None)];
        let mut disabled_target = test_target("disabled-model", "direct_model");
        disabled_target.enabled = false;
        let targets = vec![disabled_target];
        let target_ids = vec!["disabled-model".to_string()];
        let err = validate_target_compatibility(&pack, &tasks, &targets, &target_ids)
            .expect_err("disabled targets should not be runnable");

        assert!(err.starts_with("target_disabled"), "{err}");
        assert!(err.contains("disabled-model"), "{err}");
    }

    #[test]
    fn target_runtime_preflight_allows_local_model_without_key() {
        let targets = vec![test_model_target(
            "local-llama",
            "llama-cpp-openai",
            serde_json::json!({
                "model": "local-huggingface",
                "base_url": "http://127.0.0.1:8080/v1"
            }),
        )];
        let target_ids = vec!["local-llama".to_string()];

        assert!(validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &[]).is_ok());
    }

    #[test]
    fn target_runtime_preflight_rejects_remote_compatible_without_key() {
        let targets = vec![test_model_target(
            "remote-compatible",
            "openai-compatible",
            serde_json::json!({
                "model": "remote-model",
                "base_url": "https://api.example.com/v1"
            }),
        )];
        let target_ids = vec!["remote-compatible".to_string()];
        let err = validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &[])
            .expect_err("remote compatible targets without keys should fail preflight");

        assert!(err.contains("target_preflight_failed"));
        assert!(err.contains("missing_key"));
        assert!(err.contains("remote-compatible"));
    }

    #[test]
    fn target_runtime_preflight_allows_local_compatible_without_key() {
        let targets = vec![test_model_target(
            "local-compatible",
            "openai-compatible",
            serde_json::json!({
                "model": "local-model",
                "base_url": "http://127.0.0.1:8080/v1"
            }),
        )];
        let target_ids = vec!["local-compatible".to_string()];

        assert!(validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &[]).is_ok());
    }

    #[test]
    fn target_runtime_preflight_rejects_missing_cloud_key() {
        let missing_env =
            format!("BENCHFORGE_TEST_MISSING_KEY_{}", uuid::Uuid::new_v4()).replace('-', "_");
        let targets = vec![test_model_target(
            "cloud-openai",
            "openai",
            serde_json::json!({
                "model": "gpt-5-mini",
                "api_key_keychain": "benchforge-test-missing-key",
                "api_key_env": missing_env
            }),
        )];
        let target_ids = vec!["cloud-openai".to_string()];
        let err = validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &[])
            .expect_err("cloud targets without keys should fail preflight");

        assert!(err.contains("target_preflight_failed"));
        assert!(err.contains("missing_key"));
        assert!(err.contains("cloud-openai"));
    }

    #[test]
    fn target_runtime_preflight_rejects_remembered_validation_errors() {
        let mut target = test_model_target(
            "local-llama",
            "llama-cpp-openai",
            serde_json::json!({
                "model": "local-huggingface",
                "base_url": "http://127.0.0.1:8080/v1"
            }),
        );
        target.validation_status = Some("error".into());
        target.validation_detail = Some("endpoint_unreachable: connection refused".into());
        let targets = vec![target];
        let target_ids = vec!["local-llama".to_string()];

        let err = validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &[])
            .expect_err("remembered validation errors should block runs");

        assert!(err.contains("target_preflight_failed"));
        assert!(err.contains("target_validation_failed"));
        assert!(err.contains("endpoint_unreachable"));
        assert!(err.contains("local-llama"));
    }

    #[test]
    fn target_runtime_preflight_blocks_external_harness_without_command() {
        let targets = vec![test_harness_target(
            "evalplus-worker",
            serde_json::json!({}),
        )];
        let target_ids = vec!["evalplus-worker".to_string()];
        let tasks = vec![test_worker_harness_task(
            "evalplus-humaneval-plus",
            "evalplus",
        )];
        let err = validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &tasks)
            .expect_err("external harness targets need a configured command");

        assert!(err.contains("target_preflight_failed"));
        assert!(err.contains("configuration_missing"));
        assert!(err.contains("evalplus"));
        assert!(err.contains("evalplus-worker"));
    }

    #[test]
    fn target_runtime_preflight_allows_internal_security_worker_without_command() {
        let targets = vec![test_harness_target(
            "security-worker",
            serde_json::json!({}),
        )];
        let target_ids = vec!["security-worker".to_string()];
        let tasks = vec![test_worker_harness_task("semgrep-basic", "security")];

        assert!(validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &tasks).is_ok());
    }

    #[test]
    fn target_runtime_preflight_reports_missing_harness_tool() {
        let missing_command =
            format!("benchforge-test-missing-{}", uuid::Uuid::new_v4()).replace('-', "");
        let targets = vec![test_harness_target(
            "terminal-worker",
            serde_json::json!({"harness": {"command": [missing_command]}}),
        )];
        let target_ids = vec!["terminal-worker".to_string()];
        let tasks = vec![test_worker_harness_task(
            "terminal-bench-subset",
            "terminal-bench",
        )];
        let err = validate_target_runtime_preflight_for_tasks(&targets, &target_ids, &tasks)
            .expect_err("missing harness executable should fail preflight");

        assert!(err.contains("target_preflight_failed"));
        assert!(err.contains("tool_missing"));
        assert!(err.contains("terminal-bench"));
    }

    #[test]
    fn benchmark_harness_artifacts_redact_target_config_and_remove_private_input() {
        let conn = store::open_memory().expect("db should open");
        let target = test_harness_target(
            "worker",
            serde_json::json!({
                "harness": {
                    "command": ["benchforge-worker", "run", "--kind", "mock"],
                    "env": {
                        "authorization": "Bearer worker-secret",
                        "public_flag": "ok"
                    }
                },
                "nested": {"token": "worker-token"}
            }),
        );
        let pack = test_pack("security-defensive");
        let mut task = test_worker_harness_task("semgrep-basic", "mock");
        task.source_path = paths::repo_root()
            .join("benchmark-packs")
            .join("security-defensive")
            .join("tasks")
            .join("semgrep-basic.yaml");
        let result = run_benchmark_harness_task(&conn, &target, &pack, &task, None, &|| false)
            .expect("mock worker harness should run");

        let artifacts = store::list_artifacts(&conn, Some(&result.id)).expect("artifacts list");
        assert!(result
            .artifacts
            .contains(&"scoring-command.json".to_string()));
        let scoring_command_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "scoring_command")
            .expect("scoring command artifact should exist");
        let scoring_command_metadata: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&scoring_command_artifact.path)
                .expect("scoring command artifact should be readable"),
        )
        .expect("scoring command artifact should parse");
        assert_eq!(
            scoring_command_metadata["command"][0],
            serde_json::json!("benchforge-worker")
        );
        assert!(scoring_command_metadata["version_stdout"]
            .as_str()
            .unwrap_or("")
            .contains("benchforge-worker"));
        let target_config_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "target_config")
            .expect("target config artifact should exist");
        let target_config_text = fs::read_to_string(&target_config_artifact.path)
            .expect("target config artifact should be readable");

        assert!(target_config_text.contains("<redacted>"));
        assert!(!target_config_text.contains("worker-secret"));
        assert!(!target_config_text.contains("worker-token"));
        assert!(target_config_text.contains("public_flag"));
        assert!(!paths::runs_dir()
            .join(&result.id)
            .join("target-config.private.json")
            .exists());
        let result_json_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "result_json")
            .expect("worker result JSON artifact should exist");
        let result_json: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&result_json_artifact.path)
                .expect("worker result JSON artifact should be readable"),
        )
        .expect("worker result JSON artifact should parse");
        assert_required_v1_metrics_are_explicit(&result_json["metrics"]);
        assert!(result_json["metrics"]["wall_time_ms"].is_number());
        assert!(result_json["metrics"]["setup_time_ms"].is_null());
        assert!(result_json["metrics"]["input_tokens"].is_null());
        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("worker harness run result should persist");
        assert_eq!(record.reproducibility["sandbox_level"], 1);
        assert_eq!(
            record.reproducibility["permission_mode"],
            "worker-harness-minimal-env"
        );

        let _ = fs::remove_dir_all(paths::runs_dir().join(&result.id));
    }

    #[test]
    fn model_client_request_uses_benchmark_prompt_contract() {
        let mut task = test_task("prompt-contract", "prompt", None);
        task.prompt = "Return JSON only.".into();

        let request = ModelClientRequest::benchmark_prompt(&task);

        assert_eq!(request.task_id, "prompt-contract");
        assert_eq!(request.system_prompt.as_ref(), BENCHMARK_PROMPT_SYSTEM);
        assert_eq!(request.user_prompt.as_ref(), "Return JSON only.");
        assert_eq!(request.default_max_tokens, 512);
    }

    #[test]
    fn model_client_request_uses_code_edit_contract() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-code-edit-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("src")).expect("workspace should be created");
        fs::write(dir.join("src/main.py"), "print('broken')\n").expect("file should be written");
        fs::create_dir_all(dir.join(".git")).expect("git dir should be created");
        fs::write(dir.join(".git/ignored"), "ignore me").expect("git file should be written");

        let mut task = test_task("repo-contract", "repo_patch", Some("python"));
        task.prompt = "Fix the failing test.".into();

        let request = ModelClientRequest::code_edit(&task, &dir);

        assert_eq!(request.task_id, "repo-contract");
        assert_eq!(request.system_prompt.as_ref(), CODE_EDIT_SYSTEM);
        assert!(request
            .user_prompt
            .as_ref()
            .contains("Fix the failing test."));
        assert!(request
            .user_prompt
            .as_ref()
            .contains("Workspace files:\nsrc/main.py"));
        assert!(!request.user_prompt.as_ref().contains(".git/ignored"));
        assert_eq!(request.default_max_tokens, 4096);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn fixture_resolution_accepts_repo_fixture_root() {
        let mut task = test_task("fixture-ok", "repo_patch", Some("python"));
        task.source_path = paths::repo_root()
            .join("benchmark-packs")
            .join("quick-smoke")
            .join("tasks")
            .join("python-rate-limit.yaml");
        task.fixture = Some("../../../fixtures/python-rate-limit".into());

        let resolved = resolve_fixture(&task).expect("repo fixture should resolve");

        assert_eq!(
            resolved,
            paths::repo_root()
                .join("fixtures")
                .join("python-rate-limit")
                .canonicalize()
                .expect("fixture should exist")
        );
    }

    #[test]
    fn fixture_resolution_rejects_paths_outside_allowed_roots() {
        let mut task = test_task("fixture-escape", "repo_patch", Some("python"));
        task.source_path = paths::repo_root()
            .join("benchmark-packs")
            .join("quick-smoke")
            .join("tasks")
            .join("python-rate-limit.yaml");
        task.fixture = Some("../../../README.md".into());

        let err = resolve_fixture(&task).expect_err("outside fixture should be rejected");

        assert!(err.contains("outside allowed fixture roots"));
    }

    #[cfg(unix)]
    #[test]
    fn fixture_copy_rejects_symlinked_entries() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-fixture-symlink-{}",
            uuid::Uuid::new_v4()
        ));
        let source = root.join("fixture");
        let dest = root.join("workspace");
        fs::create_dir_all(&source).expect("fixture should be created");
        fs::write(root.join("outside.txt"), "do not copy\n").expect("outside file should exist");
        std::os::unix::fs::symlink(root.join("outside.txt"), source.join("outside-link"))
            .expect("symlink should be created");

        let err = copy_dir(&source, &dest).expect_err("symlink should be rejected");

        assert!(err.contains("fixture symlinks are not allowed"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn diff_change_stats_counts_unified_diff_body_lines() {
        let diff = "\
diff --git a/app.py b/app.py
index 1111111..2222222 100644
--- a/app.py
+++ b/app.py
@@ -1,3 +1,4 @@
-broken
+fixed
+extra
 context
diff --git a/README.md b/README.md
new file mode 100644
--- /dev/null
+++ b/README.md
@@ -0,0 +1 @@
+hello
";

        assert_eq!(
            diff_change_stats(diff),
            DiffChangeStats {
                files_changed: 2,
                lines_added: 3,
                lines_deleted: 1,
            }
        );
    }

    #[test]
    fn git_diff_includes_untracked_files_and_excludes_runtime_dirs() {
        let temp_dir = ScopedTempDir::new("benchforge-git-diff-untracked")
            .expect("temporary workspace should be created");
        let workspace = temp_dir.path();
        fs::write(workspace.join("tracked.txt"), "baseline\n")
            .expect("tracked file should be written");
        init_git(workspace, &|| false).expect("git baseline should be created");

        fs::write(workspace.join("created.py"), "print('new')\n")
            .expect("untracked source file should be written");
        fs::create_dir_all(workspace.join("node_modules/pkg"))
            .expect("node_modules should be created");
        fs::write(workspace.join("node_modules/pkg/index.js"), "generated\n")
            .expect("node_modules file should be written");
        fs::create_dir_all(workspace.join(SANDBOX_HOME_DIR)).expect("sandbox dir should exist");
        fs::write(
            workspace.join(SANDBOX_HOME_DIR).join("config"),
            "generated\n",
        )
        .expect("sandbox file should be written");

        let diff = run_git_diff(workspace, &|| false).expect("git diff should be captured");

        assert!(diff.contains("diff --git a/created.py b/created.py"));
        assert!(diff.contains("new file mode"));
        assert!(!diff.contains("node_modules"));
        assert!(!diff.contains(SANDBOX_HOME_DIR));
        assert_eq!(
            diff_change_stats(&diff),
            DiffChangeStats {
                files_changed: 1,
                lines_added: 1,
                lines_deleted: 0,
            }
        );
    }

    #[test]
    fn code_task_persists_direct_model_provider_metrics() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-code-provider-metrics-{}",
            uuid::Uuid::new_v4()
        ));
        let fixture = root.join("fixture");
        fs::create_dir_all(&fixture).expect("fixture should be created");
        fs::write(fixture.join("app.py"), "broken\n").expect("fixture file should be written");
        let task_source = root.join("task.yaml");
        fs::write(&task_source, "id: code-provider-metrics\n")
            .expect("task source should be written");

        let mut task = test_task("code-provider-metrics", "repo_patch", Some("python"));
        task.fixture = Some("fixture".into());
        task.source_path = task_source;
        task.prompt = "Change app.py so it contains fixed.".into();
        task.scoring.command = vec![
            "python3".into(),
            "-c".into(),
            "from pathlib import Path; assert Path('app.py').read_text() == 'fixed\\n'".into(),
        ];
        let pack = test_pack("quick-smoke");
        let target = test_model_target(
            "direct-code-model",
            "openai-compatible",
            serde_json::json!({
                "mock_output": "{\"edits\":[{\"path\":\"app.py\",\"content\":\"fixed\\n\"}]}",
                "mock_metrics": {
                    "prompt_tokens": 100,
                    "completion_tokens": 50,
                    "total_tokens": 150,
                    "provider_attempts": 2,
                    "provider_request_total_ms": 321,
                    "provider_model": "mock-code-model",
                    "finish_reason": "stop"
                },
                "input_price_usd_per_million_tokens": 10.0,
                "output_price_usd_per_million_tokens": 20.0
            }),
        );
        let conn = store::open_memory().expect("db should open");

        let result = run_task(&conn, &target, &pack, &task, false, 0, 1, None, &|| false)
            .expect("code task should run");

        assert_eq!(result.status, "passed");
        assert!(result
            .artifacts
            .contains(&"model-system-prompt.txt".to_string()));
        assert!(result.artifacts.contains(&"model-prompt.txt".to_string()));
        assert!(result.artifacts.contains(&"model-output.txt".to_string()));
        let artifacts = store::list_artifacts(&conn, Some(&result.id))
            .expect("artifacts should list for result");
        let model_prompt_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "model_prompt")
            .expect("model prompt artifact should persist");
        let model_prompt = fs::read_to_string(&model_prompt_artifact.path)
            .expect("model prompt artifact should be readable");
        assert!(model_prompt.contains("Change app.py so it contains fixed."));
        assert!(model_prompt.contains("Workspace files:\napp.py"));
        let system_prompt_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "model_system_prompt")
            .expect("model system prompt artifact should persist");
        assert_eq!(
            fs::read_to_string(&system_prompt_artifact.path)
                .expect("system prompt artifact should be readable"),
            CODE_EDIT_SYSTEM
        );
        let model_prompt_sha256 = checksum_text(&model_prompt);
        let code_system_prompt_sha256 = checksum_text(CODE_EDIT_SYSTEM);
        let task_prompt_sha256 = checksum_text(&task.prompt);
        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("run result should persist");
        assert_eq!(record.pass_fail, Some(true));
        assert_eq!(record.prompt_tokens, Some(100.0));
        assert_eq!(record.input_tokens, record.prompt_tokens);
        assert_eq!(record.completion_tokens, Some(50.0));
        assert_eq!(record.output_tokens, record.completion_tokens);
        assert_eq!(record.total_tokens, Some(150.0));
        assert_eq!(record.provider_attempts, Some(2.0));
        assert_eq!(record.provider_request_total_ms, Some(321.0));
        assert_eq!(record.provider_model.as_deref(), Some("mock-code-model"));
        assert_eq!(record.finish_reason.as_deref(), Some("stop"));
        assert_eq!(record.cost_usd, Some(0.002));
        assert_eq!(record.estimated_cost_usd, record.cost_usd);
        assert!(record.setup_time_ms.is_some());
        assert!(record.target_time_ms.is_some());
        assert!(record.evaluation_time_ms.is_some());
        assert!(record.model_call_wall_time_ms.is_some());
        assert_eq!(record.exit_code, Some(0.0));
        assert_eq!(record.stdout_bytes, Some(0.0));
        assert_eq!(record.stderr_bytes, Some(0.0));
        assert_eq!(record.files_changed, Some(1.0));
        assert_eq!(record.lines_added, Some(1.0));
        assert_eq!(record.lines_deleted, Some(1.0));
        assert_eq!(record.commands_observed_count, Some(1.0));
        assert_eq!(record.dangerous_command_hits, Some(0.0));
        assert_eq!(record.score_numeric, record.score);
        assert!(result
            .artifacts
            .contains(&"scoring-command.json".to_string()));
        let scoring_command_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "scoring_command")
            .expect("scoring command artifact should persist");
        let scoring_command_metadata: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&scoring_command_artifact.path)
                .expect("scoring command artifact should be readable"),
        )
        .expect("scoring command artifact should parse");
        assert_eq!(scoring_command_metadata["command"][0], "python3");
        assert_eq!(scoring_command_metadata["version_probe"][1], "--version");
        assert!(scoring_command_metadata["version_stdout"]
            .as_str()
            .unwrap_or("")
            .contains("Python"));
        assert_eq!(
            record.reproducibility["scoring_command_metadata"]["command"][0],
            "python3"
        );
        assert_eq!(
            record.reproducibility["scoring_command_metadata"]["version_probe"][1],
            "--version"
        );
        assert_eq!(
            record.reproducibility["prompts"]["hash_algorithm"],
            "sha256"
        );
        assert_eq!(
            record.reproducibility["prompts"]["task_prompt_sha256"].as_str(),
            Some(task_prompt_sha256.as_str())
        );
        assert_eq!(
            record.reproducibility["prompts"]["system_prompt_sha256"].as_str(),
            Some(code_system_prompt_sha256.as_str())
        );
        assert_eq!(
            record.reproducibility["prompts"]["user_prompt_sha256"].as_str(),
            Some(model_prompt_sha256.as_str())
        );
        assert!(!record
            .reproducibility
            .get("prompts")
            .unwrap_or(&serde_json::Value::Null)
            .to_string()
            .contains("Change app.py"));
        let workspace_git = &record.reproducibility["workspace"]["git"];
        assert_eq!(
            workspace_git["baseline_commit"]
                .as_str()
                .expect("baseline commit should be recorded")
                .len(),
            40
        );
        assert_eq!(
            workspace_git["baseline_tree"]
                .as_str()
                .expect("baseline tree should be recorded")
                .len(),
            40
        );
        assert_eq!(workspace_git["diff_includes_untracked"], true);
        assert!(workspace_git["diff_excluded_paths"]
            .as_array()
            .expect("excluded paths should be recorded")
            .iter()
            .any(|value| value.as_str() == Some(".benchforge-venv")));
        let diff_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "git_diff")
            .expect("git diff artifact should persist");
        let diff =
            fs::read_to_string(&diff_artifact.path).expect("git diff artifact should be readable");
        let diff_sha256 = checksum_text(&diff);
        assert_eq!(
            workspace_git["diff_sha256"].as_str(),
            Some(diff_sha256.as_str())
        );
        let result_json_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "result_json")
            .expect("result JSON artifact should persist");
        let result_json: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&result_json_artifact.path)
                .expect("result JSON artifact should be readable"),
        )
        .expect("result JSON artifact should parse");
        assert_required_v1_metrics_are_explicit(&result_json["metrics"]);
        assert_eq!(result_json["metrics"]["pass_fail"], true);
        assert_eq!(result_json["metrics"]["score_numeric"], 1.0);
        assert_eq!(result_json["metrics"]["input_tokens"], 100);
        assert_eq!(result_json["metrics"]["output_tokens"], 50);
        assert_eq!(result_json["metrics"]["estimated_cost_usd"], 0.002);
        assert!(result_json["metrics"]["ttft_ms"].is_null());
        assert_eq!(record.reproducibility["sandbox_level"], 1);
        assert_eq!(
            record.reproducibility["permission_mode"],
            "patch-basic-host-scoring"
        );
        assert_eq!(result_json["safety"]["sandbox_level"], 1);
        assert_eq!(
            result_json["safety"]["permission_mode"],
            "patch-basic-host-scoring"
        );
        assert_eq!(
            record.reproducibility["benchmark_pack"]["evidence_profile"],
            "empty"
        );
        assert!(record.reproducibility["benchmark_pack"]["evidence_warnings"].is_array());
        assert_eq!(
            record.reproducibility["benchmark_pack"]["calibration"]["status"],
            "uncalibrated"
        );
        assert!(
            record.reproducibility["benchmark_pack"]["calibration"]["quality_gates"].is_array()
        );
        let result_json_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "result_json")
            .expect("result JSON artifact should persist");
        let result_json: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&result_json_artifact.path)
                .expect("result JSON artifact should be readable"),
        )
        .expect("result JSON should parse");
        assert!(result_json["metrics"]["setup_time_ms"].is_number());
        assert!(result_json["metrics"]["target_time_ms"].is_number());
        assert_eq!(result_json["metrics"]["files_changed"], 1);
        assert_eq!(result_json["metrics"]["lines_added"], 1);
        assert_eq!(result_json["metrics"]["lines_deleted"], 1);
        assert_eq!(result_json["metrics"]["commands_observed_count"], 1);
        assert_eq!(result_json["metrics"]["dangerous_command_hits"], 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn code_task_persists_cli_agent_execution_evidence() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-cli-agent-evidence-{}",
            uuid::Uuid::new_v4()
        ));
        let fixture = root.join("fixture");
        fs::create_dir_all(&fixture).expect("fixture should be created");
        fs::write(fixture.join("app.py"), "broken\n").expect("fixture file should be written");
        let task_source = root.join("task.yaml");
        fs::write(&task_source, "id: cli-agent-evidence\n").expect("task source should be written");

        let mut task = test_task("cli-agent-evidence", "repo_patch", Some("python"));
        task.fixture = Some("fixture".into());
        task.source_path = task_source;
        task.prompt = "Create agent.txt with the configured model name.".into();
        task.scoring.command = vec![
            "python3".into(),
            "-c".into(),
            "from pathlib import Path; assert Path('agent.txt').read_text() == 'cli-test-model\\n'"
                .into(),
        ];
        let pack = test_pack("quick-smoke");
        let target = store::TargetRecord {
            id: "custom-cli-agent".into(),
            name: "Custom CLI Agent".into(),
            kind: "cli_agent".into(),
            adapter_id: "custom-cli".into(),
            config_json: serde_json::json!({
                "command": "python3",
                "args": [
                    "-c",
                    "from pathlib import Path; import os, sys; Path('agent.txt').write_text(os.environ['BENCHFORGE_TEST_MODEL'] + '\\n'); print('cli stdout ' + os.environ['BENCHFORGE_TEST_FLAG']); print('cli stderr', file=sys.stderr)",
                    "{{prompt}}"
                ],
                "working_dir": "{{workspace}}",
                "env": {
                    "BENCHFORGE_TEST_MODEL": "{{model}}",
                    "BENCHFORGE_TEST_FLAG": "{{flag}}"
                },
                "validation": {"command_args": ["--version"]},
                "model": "cli-test-model",
                "flag": "ok"
            })
            .to_string(),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        };
        let conn = store::open_memory().expect("db should open");

        let result = run_task(&conn, &target, &pack, &task, false, 0, 1, None, &|| false)
            .expect("CLI agent task should run");

        assert_eq!(result.status, "passed");
        assert!(result.artifacts.contains(&"cli-stdout.txt".to_string()));
        assert!(result.artifacts.contains(&"cli-stderr.txt".to_string()));
        assert!(result
            .artifacts
            .contains(&"cli-agent-command.json".to_string()));
        let artifacts = store::list_artifacts(&conn, Some(&result.id))
            .expect("artifacts should list for CLI result");
        let cli_stdout_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "cli_stdout")
            .expect("CLI stdout artifact should persist");
        assert_eq!(
            fs::read_to_string(&cli_stdout_artifact.path)
                .expect("CLI stdout artifact should be readable"),
            "cli stdout ok\n"
        );
        let cli_stderr_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "cli_stderr")
            .expect("CLI stderr artifact should persist");
        assert_eq!(
            fs::read_to_string(&cli_stderr_artifact.path)
                .expect("CLI stderr artifact should be readable"),
            "cli stderr\n"
        );
        let cli_command_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "cli_agent_command")
            .expect("CLI command artifact should persist");
        let command_evidence: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&cli_command_artifact.path)
                .expect("CLI command artifact should be readable"),
        )
        .expect("CLI command artifact should parse");
        assert_eq!(
            command_evidence["command_metadata"]["command"][0],
            "python3"
        );
        assert_eq!(
            command_evidence["command_metadata"]["command"][3],
            "<task_prompt>"
        );
        assert_eq!(
            command_evidence["command_metadata"]["version_probe"],
            serde_json::json!(["python3", "--version"])
        );
        assert!(command_evidence["command_metadata"]["version_stdout"]
            .as_str()
            .unwrap_or("")
            .contains("Python"));
        assert_eq!(
            command_evidence["env"]["BENCHFORGE_TEST_MODEL"],
            "cli-test-model"
        );
        assert_eq!(command_evidence["exit_code"], 0);
        assert_eq!(command_evidence["timed_out"], false);
        assert!(command_evidence["stdout_sha256"].as_str().is_some());
        assert!(!command_evidence
            .to_string()
            .contains("Create agent.txt with the configured model name."));

        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("run result should persist");
        assert_eq!(
            record.reproducibility["cli_agent"]["command_metadata"]["command"][3],
            "<task_prompt>"
        );
        let task_prompt_sha256 = checksum_text(&task.prompt);
        assert_eq!(
            record.reproducibility["prompts"]["task_prompt_sha256"].as_str(),
            Some(task_prompt_sha256.as_str())
        );
        let result_json_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "result_json")
            .expect("result JSON artifact should persist");
        let result_json: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&result_json_artifact.path)
                .expect("result JSON artifact should be readable"),
        )
        .expect("result JSON should parse");
        assert_eq!(result_json["metrics"]["cli_agent_exit_code"], 0);
        assert_eq!(result_json["metrics"]["cli_agent_timed_out"], false);
        assert!(result_json["metrics"]["cli_agent_stdout_bytes"]
            .as_u64()
            .is_some_and(|value| value > 0));
        let cli_exit_metric: f64 = conn
            .query_row(
                "SELECT value FROM metrics WHERE run_id = ?1 AND name = 'cli_agent_exit_code'",
                [&result.id],
                |row| row.get(0),
            )
            .expect("CLI exit metric should persist");
        assert_eq!(cli_exit_metric, 0.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn code_task_persists_dangerous_command_hit_count() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-dangerous-command-metrics-{}",
            uuid::Uuid::new_v4()
        ));
        let fixture = root.join("fixture");
        fs::create_dir_all(&fixture).expect("fixture should be created");
        fs::write(fixture.join("app.py"), "broken\n").expect("fixture file should be written");
        let task_source = root.join("task.yaml");
        fs::write(&task_source, "id: dangerous-command-metrics\n")
            .expect("task source should be written");

        let mut task = test_task("dangerous-command-metrics", "repo_patch", Some("python"));
        task.fixture = Some("fixture".into());
        task.source_path = task_source;
        task.prompt = "Change app.py so it contains fixed.".into();
        task.scoring.command = vec![
            "python3".into(),
            "-c".into(),
            "from pathlib import Path; assert Path('app.py').read_text() == 'fixed\\n'; print('sudo rm -rf /')".into(),
        ];
        let pack = test_pack("quick-smoke");
        let target = test_model_target(
            "direct-code-model",
            "openai-compatible",
            serde_json::json!({
                "mock_output": "{\"edits\":[{\"path\":\"app.py\",\"content\":\"fixed\\n\"}]}",
                "mock_metrics": {"provider_model": "mock-code-model"}
            }),
        );
        let conn = store::open_memory().expect("db should open");

        let result = run_task(&conn, &target, &pack, &task, false, 0, 1, None, &|| false)
            .expect("code task should run");

        assert_eq!(result.status, "passed");
        assert!(result.warnings.contains(&"rm -rf /".to_string()));
        assert!(result.warnings.contains(&"sudo".to_string()));
        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("run result should persist");
        assert_eq!(record.dangerous_command_hits, Some(2.0));
        let artifacts = store::list_artifacts(&conn, Some(&result.id))
            .expect("artifacts should list for result");
        let result_json_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "result_json")
            .expect("result JSON artifact should persist");
        let result_json: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&result_json_artifact.path)
                .expect("result JSON artifact should be readable"),
        )
        .expect("result JSON should parse");
        assert_eq!(result_json["metrics"]["dangerous_command_hits"], 2);
        assert_eq!(
            result_json["safety"]["dangerous_command_hits"],
            serde_json::json!(["rm -rf /", "sudo"])
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn provider_model_metric_falls_back_to_configured_model_only_when_missing() {
        let config = serde_json::json!({"model": "configured-model"});
        let mut missing = serde_json::Map::new();
        ensure_provider_model_metric(&mut missing, &config);
        assert_eq!(
            missing.get("provider_model"),
            Some(&serde_json::json!("configured-model"))
        );
        assert_eq!(
            missing.get("provider_model_source"),
            Some(&serde_json::json!("target_config"))
        );

        let mut reported = serde_json::Map::new();
        reported.insert("provider_model".into(), serde_json::json!("served-model"));
        ensure_provider_model_metric(&mut reported, &config);
        assert_eq!(
            reported.get("provider_model"),
            Some(&serde_json::json!("served-model"))
        );
        assert_eq!(
            reported.get("provider_model_source"),
            Some(&serde_json::json!("provider"))
        );
    }

    #[test]
    fn openai_runtime_model_ids_accepts_common_model_list_shapes() {
        let openai = serde_json::json!({
            "data": [
                {"id": "openai-style"},
                {"name": "name-style"},
                {"model": "model-style"}
            ]
        });
        assert_eq!(
            openai_runtime_model_ids(&openai),
            vec!["openai-style", "name-style", "model-style"]
        );

        let native = serde_json::json!({
            "models": [
                "string-style",
                {"name": "ollama-style"},
                {"model": "mlx-style"}
            ]
        });
        assert_eq!(
            openai_runtime_model_ids(&native),
            vec!["string-style", "ollama-style", "mlx-style"]
        );
    }

    #[test]
    fn local_openai_models_endpoint_confirms_missing_provider_model_identity() {
        let server = CloudContractServer::start().expect("contract server should start");
        let conn = store::open_memory().expect("db should open");
        let pack = load_pack("cloud-contract").expect("cloud contract pack should load");
        let mut tasks = load_tasks(&pack).expect("cloud contract task should load");
        let task = tasks.pop().expect("cloud contract task should exist");
        let target = test_model_target(
            "contract-no-model-echo",
            "openai-compatible",
            serde_json::json!({
                "model": "contract-no-model-echo",
                "base_url": format!("{}/v1", server.base_url()),
                "retry_count": 0,
                "timeout_seconds": 10,
                "max_tokens": 16
            }),
        );

        let result = run_prompt_task(&conn, &target, &pack, &task, 0, 1, None, &|| false)
            .expect("local prompt run should persist");
        assert_eq!(
            result.status,
            "passed",
            "local prompt run should pass; error: {:?}",
            result.error
        );

        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.target_id == "contract-no-model-echo")
            .expect("contract result should persist");
        assert_eq!(
            record.provider_model.as_deref(),
            Some("contract-no-model-echo")
        );
        assert_eq!(
            record.provider_model_source.as_deref(),
            Some("runtime_models")
        );
    }

    #[test]
    fn code_task_uses_configured_model_when_provider_model_metric_is_missing() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-code-provider-model-fallback-{}",
            uuid::Uuid::new_v4()
        ));
        let fixture = root.join("fixture");
        fs::create_dir_all(&fixture).expect("fixture should be created");
        fs::write(fixture.join("app.py"), "broken\n").expect("fixture file should be written");
        let task_source = root.join("task.yaml");
        fs::write(&task_source, "id: code-provider-model-fallback\n")
            .expect("task source should be written");

        let mut task = test_task("code-provider-model-fallback", "repo_patch", Some("python"));
        task.fixture = Some("fixture".into());
        task.source_path = task_source;
        task.prompt = "Change app.py so it contains fixed.".into();
        task.scoring.command = vec![
            "python3".into(),
            "-c".into(),
            "from pathlib import Path; assert Path('app.py').read_text() == 'fixed\\n'".into(),
        ];
        let pack = test_pack("quick-smoke");
        let target = test_model_target(
            "direct-code-model-fallback",
            "openai-compatible",
            serde_json::json!({
                "model": "configured-code-model",
                "mock_output": "{\"edits\":[{\"path\":\"app.py\",\"content\":\"fixed\\n\"}]}",
                "mock_metrics": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15,
                    "provider_attempts": 1,
                    "finish_reason": "stop"
                }
            }),
        );
        let conn = store::open_memory().expect("db should open");

        let result = run_task(&conn, &target, &pack, &task, false, 0, 1, None, &|| false)
            .expect("code task should run");

        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("run result should persist");
        assert_eq!(
            record.provider_model.as_deref(),
            Some("configured-code-model")
        );
        assert_eq!(
            record.provider_model_source.as_deref(),
            Some("target_config")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn code_task_persists_provider_error_when_model_invocation_fails() {
        let server = CloudContractServer::start().expect("contract server should start");
        let _api_key = ScopedEnvVar::set(CLOUD_CONTRACT_API_KEY_ENV, "benchforge-contract-key");
        let root = std::env::temp_dir().join(format!(
            "benchforge-code-provider-error-{}",
            uuid::Uuid::new_v4()
        ));
        let fixture = root.join("fixture");
        fs::create_dir_all(&fixture).expect("fixture should be created");
        fs::write(fixture.join("app.py"), "already fine\n")
            .expect("fixture file should be written");
        let task_source = root.join("task.yaml");
        fs::write(&task_source, "id: code-provider-error\n")
            .expect("task source should be written");

        let mut task = test_task("code-provider-error", "repo_patch", Some("python"));
        task.fixture = Some("fixture".into());
        task.source_path = task_source;
        task.prompt = "Return a patch.".into();
        task.scoring.command = vec![
            "python3".into(),
            "-c".into(),
            "from pathlib import Path; assert Path('app.py').read_text() == 'already fine\\n'"
                .into(),
        ];
        let pack = test_pack("quick-smoke");
        let target = test_model_target(
            "direct-code-rate-limit",
            "openai-compatible",
            serde_json::json!({
                "model": "contract-rate-limit",
                "base_url": format!("{}/v1", server.base_url()),
                "api_key_env": CLOUD_CONTRACT_API_KEY_ENV,
                "retry_count": 0,
                "timeout_seconds": 10,
                "max_tokens": 16
            }),
        );
        let conn = store::open_memory().expect("db should open");

        let result = run_task(&conn, &target, &pack, &task, false, 0, 1, None, &|| false)
            .expect("provider failure should persist as a run result");

        assert_eq!(result.status, "error");
        assert_eq!(result.score, Some(0.0));
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("http_status 429"));
        assert!(result
            .artifacts
            .contains(&"model-system-prompt.txt".to_string()));
        assert!(result.artifacts.contains(&"model-prompt.txt".to_string()));
        assert!(!result.artifacts.contains(&"model-output.txt".to_string()));
        let artifacts = store::list_artifacts(&conn, Some(&result.id))
            .expect("artifacts should list for failed result");
        let model_prompt_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "model_prompt")
            .expect("model prompt artifact should persist for provider failures");
        let model_prompt = fs::read_to_string(&model_prompt_artifact.path)
            .expect("model prompt artifact should be readable");
        assert!(model_prompt.contains("Return a patch."));
        assert!(model_prompt.contains("Workspace files:\napp.py"));
        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("run result should persist");
        assert_eq!(record.status, "error");
        assert_eq!(record.error_code.as_deref(), Some("rate_limit"));
        assert_eq!(record.http_status, Some(429.0));
        assert_eq!(record.provider_retry_after_ms, Some(2_000.0));
        assert_eq!(record.score, Some(0.0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_client_routes_supported_model_adapters() {
        let cases = [
            (
                test_model_target("local-llama", "llama-cpp-openai", serde_json::json!({})),
                "openai_compatible",
            ),
            (
                test_model_target("openrouter", "openrouter", serde_json::json!({})),
                "openai_compatible",
            ),
            (
                test_model_target("mistral", "mistral", serde_json::json!({})),
                "openai_compatible",
            ),
            (
                test_model_target("openai", "openai", serde_json::json!({})),
                "openai_responses",
            ),
            (
                test_model_target("anthropic", "anthropic", serde_json::json!({})),
                "anthropic_messages",
            ),
            (
                test_model_target("azure", "azure-openai", serde_json::json!({})),
                "azure_openai",
            ),
            (
                test_model_target("gemini", "gemini", serde_json::json!({})),
                "openai_compatible",
            ),
        ];

        for (target, expected_contract) in cases {
            let client = ModelClient::for_target(&target).expect("client should resolve");
            assert_eq!(client.contract_id(), expected_contract);
        }

        let mock = ModelClient::for_target(&test_target("mock-agent", "mock"))
            .expect("mock client should resolve");
        assert_eq!(mock.contract_id(), "mock");
    }

    #[test]
    fn packs_load_from_disk() {
        let packs = list_benchmark_packs().expect("packs should load");
        assert!(packs.iter().any(|pack| pack.id == "quick-smoke"));
        assert!(packs.iter().any(|pack| pack.id == "llm-basics"));
        assert!(packs.iter().any(|pack| pack.id == "llm-core"));
        let quick_smoke = packs
            .iter()
            .find(|pack| pack.id == "quick-smoke")
            .expect("quick-smoke should load");
        assert_eq!(quick_smoke.source, "built-in");
        assert_eq!(quick_smoke.evidence_profile, "code_smoke");
        assert!(quick_smoke
            .evidence_warnings
            .iter()
            .any(|warning| warning.contains("Smoke packs")));
        assert!(quick_smoke
            .source_path
            .ends_with("benchmark-packs/quick-smoke"));
        let code_edit = packs
            .iter()
            .find(|pack| pack.id == "code-edit-core")
            .expect("code-edit-core should load");
        assert_eq!(code_edit.task_types, vec!["repo_patch".to_string()]);
        assert!(code_edit
            .supported_target_kinds
            .contains(&"direct_model".to_string()));
        assert!(code_edit.required_tools.contains(&"npm".to_string()));
        assert!(code_edit.required_tools.contains(&"python".to_string()));
        let basics = packs
            .iter()
            .find(|pack| pack.id == "llm-basics")
            .expect("llm-basics should load");
        assert_eq!(basics.task_types, vec!["prompt".to_string()]);
        assert_eq!(basics.prompt_tasks, 3);
        assert_eq!(basics.total_task_weight, 3.0);
        assert_eq!(basics.evidence_profile, "prompt_comparison");
        assert!(basics.evidence_warnings.is_empty());
        assert_eq!(basics.calibration_status, "pilot");
        assert_eq!(basics.calibration_sample_size, Some(0));
        assert_eq!(
            basics.calibration_last_reviewed.as_deref(),
            Some("2026-07-08")
        );
        assert_eq!(
            basics.calibration_review_scope.as_deref(),
            Some("contract_review")
        );
        assert!(basics
            .calibration_quality_gates
            .contains(&"local_cloud_baseline_pair".to_string()));
        assert!(basics
            .calibration_quality_gates
            .contains(&"min_3_repetitions_per_task_target".to_string()));
        assert!(basics
            .calibration_notes
            .as_deref()
            .unwrap_or_default()
            .contains("not empirically calibrated"));
        assert!(basics
            .supported_target_kinds
            .contains(&"direct_model".to_string()));
        assert!(basics
            .supported_target_kinds
            .contains(&"harnessed_model".to_string()));
        assert!(basics.target_fit.contains("Local/cloud"));
        let connectivity = packs
            .iter()
            .find(|pack| pack.id == "llm-connectivity")
            .expect("llm-connectivity should load");
        assert_eq!(connectivity.prompt_tasks, 2);
        assert_eq!(connectivity.evidence_profile, "connectivity_smoke");
        assert!(connectivity
            .evidence_warnings
            .iter()
            .any(|warning| warning.contains("endpoint response")));
        let practical = packs
            .iter()
            .find(|pack| pack.id == "llm-practical")
            .expect("llm-practical should load");
        assert!(practical
            .scoring_methods
            .contains(&"exact JSON arrays".to_string()));
        assert!(practical
            .scoring_methods
            .contains(&"numeric bounds".to_string()));
        let structured = packs
            .iter()
            .find(|pack| pack.id == "llm-structured-output")
            .expect("llm-structured-output should load");
        assert!(structured
            .scoring_methods
            .contains(&"exact JSON object keys".to_string()));
        assert!(structured
            .scoring_methods
            .contains(&"ordered JSON arrays".to_string()));
    }

    #[test]
    fn user_benchmark_pack_roots_are_discoverable_and_runnable() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-user-pack-root-{}",
            uuid::Uuid::new_v4()
        ));
        write_test_prompt_pack(&root, "private-eval", "private-eval-ok-001", "OK");
        let roots = vec![BenchmarkPackRoot {
            path: root.clone(),
            source: "user",
            required: true,
        }];
        let discovered =
            find_benchmark_pack_path("private-eval", &roots).expect("private pack should resolve");
        let pack = load_pack_from_path_with_source(&discovered.path, discovered.source)
            .expect("private pack should load");
        assert_eq!(pack.source, "user");
        assert!(pack.pack_dir.ends_with("private-eval"));
        let tasks = load_tasks(&pack).expect("private tasks should load");
        assert_eq!(tasks.len(), 1);
        let canonical_pack_dir = root
            .join("private-eval")
            .canonicalize()
            .expect("private pack dir should canonicalize");
        assert!(tasks[0].source_path.starts_with(canonical_pack_dir));

        let conn = store::open_memory().expect("db should open");
        let target = test_target("mock-agent", "mock");
        let result = run_prompt_task(&conn, &target, &pack, &tasks[0], 0, 1, None, &|| false)
            .expect("private prompt task should run");
        assert_eq!(result.status, "passed");
        assert_eq!(result.score, Some(1.0));
        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("private run should persist");
        assert_eq!(
            record
                .reproducibility
                .pointer("/benchmark_pack/source")
                .and_then(|value| value.as_str()),
            Some("user")
        );
        assert_eq!(record.reproducibility["sandbox_level"], 0);
        assert_eq!(record.reproducibility["permission_mode"], "safe-readonly");
        let task_prompt_sha256 = checksum_text(&tasks[0].prompt);
        let system_prompt_sha256 = checksum_text(BENCHMARK_PROMPT_SYSTEM);
        assert_eq!(
            record.reproducibility["prompts"]["hash_algorithm"],
            "sha256"
        );
        assert_eq!(
            record.reproducibility["prompts"]["task_prompt_sha256"].as_str(),
            Some(task_prompt_sha256.as_str())
        );
        assert_eq!(
            record.reproducibility["prompts"]["system_prompt_sha256"].as_str(),
            Some(system_prompt_sha256.as_str())
        );
        assert_eq!(
            record.reproducibility["prompts"]["user_prompt_sha256"].as_str(),
            Some(task_prompt_sha256.as_str())
        );
        assert!(!record
            .reproducibility
            .get("prompts")
            .unwrap_or(&serde_json::Value::Null)
            .to_string()
            .contains("Return exactly"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_template_creation_writes_loadable_user_pack() {
        let root =
            std::env::temp_dir().join(format!("benchforge-pack-template-{}", uuid::Uuid::new_v4()));
        let created = create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-template".into(),
                name: "Private Template".into(),
                description: Some("Private prompt checks.".into()),
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        assert_eq!(created.pack.id, "private-template");
        assert_eq!(created.pack.source, "user");
        assert_eq!(created.pack.tasks, 1);
        assert_eq!(created.pack.calibration_status, "uncalibrated");
        assert!(created
            .pack
            .calibration_notes
            .as_deref()
            .unwrap_or_default()
            .contains("Starter private pack"));
        assert!(created.pack.scoring_methods.contains(&"exact".to_string()));
        assert!(Path::new(&created.source_path).join("pack.yaml").exists());
        assert!(Path::new(&created.task_path).exists());

        let roots = vec![BenchmarkPackRoot {
            path: root.clone(),
            source: "user",
            required: true,
        }];
        let scan = scan_benchmark_packs(&roots);
        assert!(scan.packs.iter().any(|pack| pack.id == "private-template"));
        assert!(scan
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.status == "ok"));

        let discovered = find_benchmark_pack_path("private-template", &roots)
            .expect("created pack should resolve");
        let pack = load_pack_from_path_with_source(&discovered.path, discovered.source)
            .expect("created pack should load");
        let tasks = load_tasks(&pack).expect("created task should load");
        let conn = store::open_memory().expect("db should open");
        let target = test_target("mock-agent", "mock");
        let result = run_prompt_task(&conn, &target, &pack, &tasks[0], 0, 1, None, &|| false)
            .expect("created prompt task should run");
        assert_eq!(result.status, "passed");
        assert_eq!(result.score, Some(1.0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_calibration_update_writes_user_pack_metadata() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-calibration-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-calibration".into(),
                name: "Private Calibration".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let updated = update_benchmark_pack_calibration_in_root(
            &root,
            UpdateBenchmarkPackCalibrationRequest {
                pack_id: "private-calibration".into(),
                status: "reviewed".into(),
                sample_size: Some(6),
                baseline_models: vec![
                    "local-qwen".into(),
                    " cloud-mini ".into(),
                    "local-qwen".into(),
                ],
                last_reviewed: Some("2026-07-07".into()),
                notes: Some("Reviewed after pilot local/cloud run.".into()),
            },
        )
        .expect("calibration should update");

        assert_eq!(updated.pack.calibration_status, "reviewed");
        assert_eq!(updated.pack.calibration_sample_size, Some(6));
        assert_eq!(
            updated.pack.calibration_baseline_models,
            vec!["cloud-mini".to_string(), "local-qwen".to_string()]
        );
        assert_eq!(
            updated.pack.calibration_last_reviewed.as_deref(),
            Some("2026-07-07")
        );
        assert!(updated
            .pack
            .calibration_notes
            .as_deref()
            .unwrap_or_default()
            .contains("pilot local/cloud"));

        let pack = load_pack_from_path_with_source(
            &root.join("private-calibration").join("pack.yaml"),
            "user",
        )
        .expect("updated pack should load");
        assert_eq!(
            pack.calibration
                .as_ref()
                .map(|calibration| calibration.status.as_str()),
            Some("reviewed")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_calibration_update_rejects_unproven_calibrated_status() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-calibration-reject-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-calibration".into(),
                name: "Private Calibration".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let err = update_benchmark_pack_calibration_in_root(
            &root,
            UpdateBenchmarkPackCalibrationRequest {
                pack_id: "private-calibration".into(),
                status: "calibrated".into(),
                sample_size: Some(0),
                baseline_models: Vec::new(),
                last_reviewed: None,
                notes: None,
            },
        )
        .expect_err("calibrated status should require provenance");
        assert!(err.contains("positive sample size"));

        let err = update_benchmark_pack_calibration_in_root(
            &root,
            UpdateBenchmarkPackCalibrationRequest {
                pack_id: "private-calibration".into(),
                status: "calibrated".into(),
                sample_size: Some(12),
                baseline_models: vec!["local-baseline".into()],
                last_reviewed: Some("2026-07-07".into()),
                notes: Some("Reviewed local baseline only.".into()),
            },
        )
        .expect_err("calibrated status should require multiple baselines");
        assert!(err.contains("at least two baseline models"));

        let err = update_benchmark_pack_calibration_in_root(
            &root,
            UpdateBenchmarkPackCalibrationRequest {
                pack_id: "private-calibration".into(),
                status: "calibrated".into(),
                sample_size: Some(12),
                baseline_models: vec!["local-baseline".into(), "cloud-baseline".into()],
                last_reviewed: Some("2026-07-07".into()),
                notes: None,
            },
        )
        .expect_err("calibrated status should require review notes");
        assert!(err.contains("review notes"));

        let updated = update_benchmark_pack_calibration_in_root(
            &root,
            UpdateBenchmarkPackCalibrationRequest {
                pack_id: "private-calibration".into(),
                status: "calibrated".into(),
                sample_size: Some(12),
                baseline_models: vec!["local-baseline".into(), "cloud-baseline".into()],
                last_reviewed: Some("2026-07-07".into()),
                notes: Some("Reviewed local and cloud baseline model runs.".into()),
            },
        )
        .expect("complete calibrated provenance should save");
        assert_eq!(updated.pack.calibration_status, "calibrated");
        assert_eq!(updated.pack.calibration_baseline_models.len(), 2);

        let err = update_benchmark_pack_calibration_in_root(
            &root,
            UpdateBenchmarkPackCalibrationRequest {
                pack_id: "private-calibration".into(),
                status: "pilot".into(),
                sample_size: Some(1),
                baseline_models: vec!["baseline".into()],
                last_reviewed: Some("2026-02-30".into()),
                notes: None,
            },
        )
        .expect_err("invalid review date should fail");
        assert!(err.contains("valid YYYY-MM-DD"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_template_creation_refuses_existing_pack_dir() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-template-existing-{}",
            uuid::Uuid::new_v4()
        ));
        write_test_prompt_pack(&root, "existing-private", "existing-private-001", "OK");
        let err = create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "existing-private".into(),
                name: "Existing Private".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect_err("existing pack dir should not be overwritten");
        assert!(err.contains("already exists"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_prompt_task_append_updates_user_pack() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-task-append-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let added = add_benchmark_pack_prompt_task_in_root(
            &root,
            AddBenchmarkPackPromptTaskRequest {
                pack_id: "private-suite".into(),
                task_id: None,
                name: "Mention local and cloud".into(),
                prompt: "Reply with a sentence that includes local and cloud.".into(),
                scoring_method: "contains".into(),
                expected_response: Some("local\ncloud".into()),
                timeout_seconds: Some(90),
                weight: Some(2.0),
            },
        )
        .expect("prompt task should append");

        assert_eq!(added.pack.id, "private-suite");
        assert_eq!(added.pack.tasks, 2);
        assert!(added.pack.scoring_methods.contains(&"contains".to_string()));
        assert!(Path::new(&added.task_path).exists());

        let pack_path = root.join("private-suite").join("pack.yaml");
        let raw_pack = fs::read_to_string(&pack_path).expect("pack should read");
        assert!(raw_pack.contains(&format!("tasks/{}.yaml", added.task_id)));

        let pack = load_pack_from_path_with_source(&pack_path, "user").expect("pack should load");
        let tasks = load_tasks(&pack).expect("tasks should load");
        let appended = tasks
            .iter()
            .find(|task| task.id == added.task_id)
            .expect("appended task should load");
        assert_eq!(appended.timeout_seconds, 90);
        assert_eq!(appended.weight, 2.0);
        assert_eq!(
            appended.scoring.expect_contains,
            vec!["local".to_string(), "cloud".to_string()]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_task_listing_exposes_prompt_and_scoring_details() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-task-list-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");
        let added = add_benchmark_pack_prompt_task_in_root(
            &root,
            AddBenchmarkPackPromptTaskRequest {
                pack_id: "private-suite".into(),
                task_id: Some("private-suite-json-001".into()),
                name: "Private JSON".into(),
                prompt: "Return valid JSON.".into(),
                scoring_method: "json".into(),
                expected_response: None,
                timeout_seconds: Some(75),
                weight: Some(3.0),
            },
        )
        .expect("task should append");

        let pack =
            load_pack_from_path_with_source(&root.join("private-suite").join("pack.yaml"), "user")
                .expect("pack should load");
        let tasks = load_tasks(&pack).expect("tasks should load");
        let dtos: Vec<BenchmarkPackTaskDto> = tasks.iter().map(benchmark_pack_task_dto).collect();
        assert_eq!(dtos.len(), 2);
        let json_task = dtos
            .iter()
            .find(|task| task.id == added.task_id)
            .expect("json task should list");
        assert_eq!(json_task.prompt, "Return valid JSON.");
        assert_eq!(json_task.timeout_seconds, 75);
        assert_eq!(json_task.weight, 3.0);
        assert!(json_task.scoring_methods.contains(&"json".to_string()));
        assert_eq!(
            json_task
                .scoring
                .get("expect_json")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert!(Path::new(&json_task.source_path).exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_prompt_task_append_supports_structured_json_scoring() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-structured-task-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let added = add_benchmark_pack_prompt_task_in_root(
            &root,
            AddBenchmarkPackPromptTaskRequest {
                pack_id: "private-suite".into(),
                task_id: Some("private-suite-structured-001".into()),
                name: "Structured".into(),
                prompt: "Return compact JSON.".into(),
                scoring_method: "json_field_equals".into(),
                expected_response: Some(
                    r#"{"status":"ok","allowed":true,"metrics.count":3}"#.into(),
                ),
                timeout_seconds: Some(120),
                weight: Some(1.5),
            },
        )
        .expect("structured prompt task should append");

        let pack =
            load_pack_from_path_with_source(&root.join("private-suite").join("pack.yaml"), "user")
                .expect("pack should load");
        let tasks = load_tasks(&pack).expect("tasks should load");
        let task = tasks
            .iter()
            .find(|task| task.id == added.task_id)
            .expect("structured task should load");
        assert!(task.scoring.expect_json);
        assert_eq!(
            task.scoring.json_field_equals.get("status"),
            Some(&serde_json::json!("ok"))
        );
        assert_eq!(
            task.scoring.json_field_equals.get("allowed"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            task.scoring.json_field_equals.get("metrics.count"),
            Some(&serde_json::json!(3))
        );
        assert!(task
            .scoring
            .json_field_number_bounds
            .get("metrics.count")
            .is_none());
        assert_eq!(task.weight, 1.5);

        let result = score_prompt_response(
            &task.scoring,
            r#"{"status":"ok","allowed":true,"metrics":{"count":3}}"#,
        );
        assert_eq!(result.status, "passed");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_prompt_task_append_rejects_bad_structured_json_scoring() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-bad-structured-task-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let err = add_benchmark_pack_prompt_task_in_root(
            &root,
            AddBenchmarkPackPromptTaskRequest {
                pack_id: "private-suite".into(),
                task_id: Some("private-suite-bad-structured-001".into()),
                name: "Bad Structured".into(),
                prompt: "Return compact JSON.".into(),
                scoring_method: "json_field_number_bounds".into(),
                expected_response: Some(r#"{"cost":{"min":10,"max":1}}"#.into()),
                timeout_seconds: None,
                weight: None,
            },
        )
        .expect_err("invalid structured scoring should be rejected");

        assert!(err.contains("min for cost must be <= max"));
        assert!(!root
            .join("private-suite")
            .join("tasks/private-suite-bad-structured-001.yaml")
            .exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_prompt_task_update_overwrites_existing_user_task() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-task-update-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let updated = update_benchmark_pack_prompt_task_in_root(
            &root,
            UpdateBenchmarkPackPromptTaskRequest {
                pack_id: "private-suite".into(),
                task_id: "private-suite-prompt-001".into(),
                name: "Updated prompt".into(),
                prompt: "Return compact JSON with status ok.".into(),
                scoring_method: "json_field_equals".into(),
                expected_response: Some(r#"{"status":"ok"}"#.into()),
                timeout_seconds: Some(45),
                weight: Some(2.5),
            },
        )
        .expect("prompt task should update");

        assert_eq!(updated.task_id, "private-suite-prompt-001");
        let pack =
            load_pack_from_path_with_source(&root.join("private-suite").join("pack.yaml"), "user")
                .expect("pack should load");
        let tasks = load_tasks(&pack).expect("tasks should load");
        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];
        assert_eq!(task.id, "private-suite-prompt-001");
        assert_eq!(task.name, "Updated prompt");
        assert_eq!(task.prompt, "Return compact JSON with status ok.");
        assert_eq!(task.timeout_seconds, 45);
        assert_eq!(task.weight, 2.5);
        assert!(task.scoring.expect_json);
        assert_eq!(
            task.scoring.json_field_equals.get("status"),
            Some(&serde_json::json!("ok"))
        );
        let score = score_prompt_response(&task.scoring, r#"{"status":"ok"}"#);
        assert_eq!(score.status, "passed");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_prompt_task_update_rejects_non_prompt_tasks() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-task-update-non-prompt-{}",
            uuid::Uuid::new_v4()
        ));
        let pack_dir = root.join("private-code");
        fs::create_dir_all(pack_dir.join("tasks")).expect("pack dir should create");
        fs::write(
            pack_dir.join("pack.yaml"),
            r#"
id: private-code
name: Private Code
version: 0.1.0
tasks:
  - tasks/code.yaml
"#,
        )
        .expect("pack should write");
        fs::write(
            pack_dir.join("tasks/code.yaml"),
            r#"
id: private-code-001
name: Private Code Task
type: repo_patch
version: 0.1.0
language: python
fixture: fixtures/app
prompt: Fix it.
scoring:
  command: ["python3", "-m", "unittest"]
"#,
        )
        .expect("task should write");

        let err = update_benchmark_pack_prompt_task_in_root(
            &root,
            UpdateBenchmarkPackPromptTaskRequest {
                pack_id: "private-code".into(),
                task_id: "private-code-001".into(),
                name: "Nope".into(),
                prompt: "Return OK.".into(),
                scoring_method: "exact".into(),
                expected_response: Some("OK".into()),
                timeout_seconds: None,
                weight: None,
            },
        )
        .expect_err("non-prompt task should not edit through prompt form");

        assert!(err.contains("only prompt tasks can be edited here"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_prompt_task_scoring_preview_uses_real_scorer() {
        let preview = score_prompt_task_preview(ScorePromptTaskPreviewRequest {
            scoring_method: "json_field_number_bounds".into(),
            expected_response: Some(r#"{"latency_ms":{"min":0,"max":5000}}"#.into()),
            sample_response: r#"{"latency_ms":4200}"#.into(),
        })
        .expect("preview should score");

        assert_eq!(preview.status, "passed");
        assert_eq!(preview.score, 1.0);
        assert!(preview
            .scoring_methods
            .contains(&"numeric bounds".to_string()));
        assert_eq!(
            preview.tests.pointer("/checks/1/kind"),
            Some(&serde_json::json!("json_field_number_bounds"))
        );

        let failed = score_prompt_task_preview(ScorePromptTaskPreviewRequest {
            scoring_method: "contains".into(),
            expected_response: Some("local\ncloud".into()),
            sample_response: "local only".into(),
        })
        .expect("preview should score failures");
        assert_eq!(failed.status, "failed");
        assert!(failed
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("contains")));
    }

    #[test]
    fn benchmark_pack_prompt_task_scoring_preview_rejects_bad_contract() {
        let err = score_prompt_task_preview(ScorePromptTaskPreviewRequest {
            scoring_method: "json_field_number_bounds".into(),
            expected_response: Some(r#"{"latency_ms":{"min":10,"max":1}}"#.into()),
            sample_response: r#"{"latency_ms":5}"#.into(),
        })
        .expect_err("invalid scoring contract should be rejected");

        assert!(err.contains("min for latency_ms must be <= max"));
    }

    #[test]
    fn benchmark_pack_task_delete_updates_user_pack_and_removes_file() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-task-delete-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");
        let added = add_benchmark_pack_prompt_task_in_root(
            &root,
            AddBenchmarkPackPromptTaskRequest {
                pack_id: "private-suite".into(),
                task_id: Some("private-suite-extra-001".into()),
                name: "Extra".into(),
                prompt: "Reply with exactly EXTRA.".into(),
                scoring_method: "exact".into(),
                expected_response: Some("EXTRA".into()),
                timeout_seconds: None,
                weight: None,
            },
        )
        .expect("task should append");
        assert!(Path::new(&added.task_path).exists());

        let deleted = delete_benchmark_pack_task_in_root(
            &root,
            DeleteBenchmarkPackTaskRequest {
                pack_id: "private-suite".into(),
                task_id: added.task_id.clone(),
            },
        )
        .expect("task should delete");

        assert_eq!(deleted.deleted_task_id, added.task_id);
        assert_eq!(deleted.pack.tasks, 1);
        assert!(!Path::new(&deleted.deleted_task_path).exists());
        let pack_path = root.join("private-suite").join("pack.yaml");
        let raw_pack = fs::read_to_string(&pack_path).expect("pack should read");
        assert!(!raw_pack.contains("private-suite-extra-001"));
        let pack = load_pack_from_path_with_source(&pack_path, "user").expect("pack should load");
        let tasks = load_tasks(&pack).expect("remaining task should load");
        assert_eq!(tasks.len(), 1);
        assert_ne!(tasks[0].id, deleted.deleted_task_id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_task_delete_refuses_last_task() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-task-delete-last-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let err = delete_benchmark_pack_task_in_root(
            &root,
            DeleteBenchmarkPackTaskRequest {
                pack_id: "private-suite".into(),
                task_id: "private-suite-prompt-001".into(),
            },
        )
        .expect_err("last task should not delete");

        assert!(err.contains("must keep at least one task"));
        assert!(root
            .join("private-suite")
            .join("tasks/private-suite-prompt-001.yaml")
            .exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_export_copies_loadable_folder() {
        let export_root =
            std::env::temp_dir().join(format!("benchforge-pack-export-{}", uuid::Uuid::new_v4()));
        let exported = export_benchmark_pack_to_root(
            ExportBenchmarkPackRequest {
                pack_id: "llm-basics".into(),
                destination_dir: None,
                format: None,
            },
            Some(&export_root),
        )
        .expect("built-in pack should export");

        assert_eq!(exported.pack.id, "llm-basics");
        assert!(exported.files_copied >= exported.pack.tasks + 1);
        let export_path = Path::new(&exported.export_path);
        assert!(export_path.starts_with(&export_root));
        assert!(export_path.join("pack.yaml").exists());
        let pack = load_pack_from_path_with_source(&export_path.join("pack.yaml"), "export")
            .expect("exported pack should load");
        let tasks = load_tasks(&pack).expect("exported tasks should load");
        assert_eq!(tasks.len(), exported.pack.tasks);

        let _ = fs::remove_dir_all(export_root);
    }

    #[test]
    fn benchmark_pack_export_writes_loadable_zip_archive() {
        let export_root = std::env::temp_dir().join(format!(
            "benchforge-pack-export-zip-{}",
            uuid::Uuid::new_v4()
        ));
        let exported = export_benchmark_pack_to_root(
            ExportBenchmarkPackRequest {
                pack_id: "llm-basics".into(),
                destination_dir: None,
                format: Some("zip".into()),
            },
            Some(&export_root),
        )
        .expect("built-in pack should export as zip");

        assert_eq!(exported.pack.id, "llm-basics");
        assert_eq!(exported.format, "zip");
        assert!(exported.files_copied >= exported.pack.tasks + 1);
        let export_path = Path::new(&exported.export_path);
        assert!(export_path.starts_with(&export_root));
        assert_eq!(
            export_path.extension().and_then(|value| value.to_str()),
            Some("zip")
        );
        assert!(export_path.is_file());

        let extract_root = export_root.join("extract");
        extract_benchmark_pack_zip(export_path, &extract_root).expect("zip should extract");
        let pack_path = find_extracted_benchmark_pack_path(&extract_root)
            .expect("extracted zip should contain a pack");
        let pack = load_pack_from_path_with_source(&pack_path, "export")
            .expect("extracted pack should load");
        let tasks = load_tasks(&pack).expect("extracted tasks should load");
        assert_eq!(tasks.len(), exported.pack.tasks);

        let _ = fs::remove_dir_all(export_root);
    }

    #[test]
    fn benchmark_pack_import_copies_folder_into_user_root() {
        let root =
            std::env::temp_dir().join(format!("benchforge-pack-import-{}", uuid::Uuid::new_v4()));
        let source_root = root.join("source");
        let user_root = root.join("user");
        write_test_prompt_pack(
            &source_root,
            "portable-private",
            "portable-private-001",
            "OK",
        );
        let existing_roots = vec![BenchmarkPackRoot {
            path: user_root.clone(),
            source: "user",
            required: false,
        }];

        let imported = import_benchmark_pack_into_root(
            &user_root,
            &existing_roots,
            ImportBenchmarkPackRequest {
                source_path: source_root
                    .join("portable-private")
                    .to_string_lossy()
                    .to_string(),
            },
        )
        .expect("private pack should import");

        assert_eq!(imported.pack.id, "portable-private");
        assert_eq!(imported.pack.source, "user");
        assert!(imported.files_copied >= 2);
        assert!(user_root
            .join("portable-private")
            .join("pack.yaml")
            .exists());
        let pack = load_pack_from_path_with_source(
            &user_root.join("portable-private").join("pack.yaml"),
            "user",
        )
        .expect("imported pack should load");
        let tasks = load_tasks(&pack).expect("imported tasks should load");
        assert_eq!(tasks.len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_import_accepts_zip_archive() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-import-zip-{}",
            uuid::Uuid::new_v4()
        ));
        let source_root = root.join("source");
        let user_root = root.join("user");
        write_test_prompt_pack(&source_root, "zip-private", "zip-private-001", "OK");
        let zip_path = root.join("zip-private.zip");
        create_benchmark_pack_zip(&source_root, &zip_path).expect("source pack folder should zip");
        let existing_roots = vec![BenchmarkPackRoot {
            path: user_root.clone(),
            source: "user",
            required: false,
        }];

        let imported = import_benchmark_pack_into_root(
            &user_root,
            &existing_roots,
            ImportBenchmarkPackRequest {
                source_path: zip_path.to_string_lossy().to_string(),
            },
        )
        .expect("private zip pack should import");

        assert_eq!(imported.pack.id, "zip-private");
        assert_eq!(imported.pack.source, "user");
        assert_eq!(
            imported.source_path,
            zip_path
                .canonicalize()
                .expect("zip path should canonicalize")
                .to_string_lossy()
        );
        assert!(imported.files_copied >= 2);
        assert!(user_root.join("zip-private").join("pack.yaml").exists());
        let pack = load_pack_from_path_with_source(
            &user_root.join("zip-private").join("pack.yaml"),
            "user",
        )
        .expect("imported zip pack should load");
        let tasks = load_tasks(&pack).expect("imported zip tasks should load");
        assert_eq!(tasks.len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_import_rejects_zip_path_traversal_entries() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-import-zip-traversal-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("zip test root should be created");
        let user_root = root.join("user");
        let zip_path = root.join("traversal-private.zip");
        let file = fs::File::create(&zip_path).expect("zip should be created");
        let mut zip = ZipWriter::new(file);
        let file_options = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        zip.start_file("pack.yaml", file_options)
            .expect("pack entry should start");
        zip.write_all(
            b"id: traversal-private\nname: Traversal Private\nversion: 0.1.0\ntasks:\n  - tasks/task.yaml\n",
        )
        .expect("pack entry should write");
        zip.start_file("tasks/task.yaml", file_options)
            .expect("task entry should start");
        zip.write_all(
            b"id: traversal-private-001\nname: Check\ntype: prompt\nprompt: Reply OK.\nscoring:\n  expect_exact: OK\n",
        )
        .expect("task entry should write");
        zip.start_file("../outside.txt", file_options)
            .expect("unsafe entry should start");
        zip.write_all(b"outside")
            .expect("unsafe entry should write");
        zip.finish().expect("zip should finish");
        let existing_roots = vec![BenchmarkPackRoot {
            path: user_root.clone(),
            source: "user",
            required: false,
        }];

        let err = import_benchmark_pack_into_root(
            &user_root,
            &existing_roots,
            ImportBenchmarkPackRequest {
                source_path: zip_path.to_string_lossy().to_string(),
            },
        )
        .expect_err("zip traversal payload should not import");

        assert!(err.contains("unsafe path"));
        assert!(!user_root.join("traversal-private").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_import_refuses_duplicate_pack_id() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-import-duplicate-{}",
            uuid::Uuid::new_v4()
        ));
        let source_root = root.join("source");
        let existing_root = root.join("existing");
        let user_root = root.join("user");
        write_test_prompt_pack(
            &source_root,
            "duplicate-private",
            "duplicate-private-001",
            "OK",
        );
        write_test_prompt_pack(
            &existing_root,
            "duplicate-private",
            "duplicate-private-existing-001",
            "OK",
        );
        let existing_roots = vec![BenchmarkPackRoot {
            path: existing_root.clone(),
            source: "user",
            required: true,
        }];

        let err = import_benchmark_pack_into_root(
            &user_root,
            &existing_roots,
            ImportBenchmarkPackRequest {
                source_path: source_root
                    .join("duplicate-private")
                    .to_string_lossy()
                    .to_string(),
            },
        )
        .expect_err("duplicate pack id should not import");

        assert!(err.contains("already exists"));
        assert!(!user_root.join("duplicate-private").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn benchmark_pack_import_rejects_symlink_payloads() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-import-symlink-{}",
            uuid::Uuid::new_v4()
        ));
        let source_root = root.join("source");
        let user_root = root.join("user");
        write_test_prompt_pack(&source_root, "symlink-private", "symlink-private-001", "OK");
        std::os::unix::fs::symlink(
            "pack.yaml",
            source_root.join("symlink-private").join("pack-link.yaml"),
        )
        .expect("symlink should create");
        let existing_roots = vec![BenchmarkPackRoot {
            path: user_root.clone(),
            source: "user",
            required: false,
        }];

        let err = import_benchmark_pack_into_root(
            &user_root,
            &existing_roots,
            ImportBenchmarkPackRequest {
                source_path: source_root
                    .join("symlink-private")
                    .to_string_lossy()
                    .to_string(),
            },
        )
        .expect_err("symlinked pack should not import");

        assert!(err.contains("symlinks are not allowed"));
        assert!(!user_root.join("symlink-private").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_prompt_task_append_refuses_duplicate_task_id() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-pack-task-dupe-{}",
            uuid::Uuid::new_v4()
        ));
        create_benchmark_pack_template_in_root(
            &root,
            CreateBenchmarkPackTemplateRequest {
                id: "private-suite".into(),
                name: "Private Suite".into(),
                description: None,
                prompt: "Reply with exactly OK.".into(),
                expected_response: "OK".into(),
            },
        )
        .expect("template should create");

        let err = add_benchmark_pack_prompt_task_in_root(
            &root,
            AddBenchmarkPackPromptTaskRequest {
                pack_id: "private-suite".into(),
                task_id: Some("private-suite-prompt-001".into()),
                name: "Duplicate".into(),
                prompt: "Reply with exactly OK.".into(),
                scoring_method: "exact".into(),
                expected_response: Some("OK".into()),
                timeout_seconds: None,
                weight: None,
            },
        )
        .expect_err("duplicate task id should be rejected");
        assert!(err.contains("already exists"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_benchmark_pack_ids_are_rejected_across_roots() {
        let first =
            std::env::temp_dir().join(format!("benchforge-pack-dupe-a-{}", uuid::Uuid::new_v4()));
        let second =
            std::env::temp_dir().join(format!("benchforge-pack-dupe-b-{}", uuid::Uuid::new_v4()));
        write_test_prompt_pack(&first, "duplicate-eval", "duplicate-a-001", "OK");
        write_test_prompt_pack(&second, "duplicate-eval", "duplicate-b-001", "OK");
        let roots = vec![
            BenchmarkPackRoot {
                path: first.clone(),
                source: "user",
                required: true,
            },
            BenchmarkPackRoot {
                path: second.clone(),
                source: "external",
                required: true,
            },
        ];
        let err = find_benchmark_pack_path("duplicate-eval", &roots)
            .expect_err("duplicate pack ids should be rejected");
        assert!(err.contains("duplicate benchmark pack id duplicate-eval"));

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn invalid_user_pack_diagnostics_do_not_hide_valid_packs() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-invalid-pack-root-{}",
            uuid::Uuid::new_v4()
        ));
        write_test_prompt_pack(&root, "valid-private-eval", "valid-private-001", "OK");
        let invalid_dir = root.join("broken-private-eval");
        fs::create_dir_all(&invalid_dir).expect("invalid dir should create");
        fs::write(
            invalid_dir.join("pack.yaml"),
            "id: broken-private-eval\nname: Broken\nversion: 0.1.0\ntasks:\n  - ../outside.yaml\n",
        )
        .expect("invalid pack should write");
        let roots = vec![BenchmarkPackRoot {
            path: root.clone(),
            source: "user",
            required: true,
        }];
        let scan = scan_benchmark_packs(&roots);
        assert!(scan
            .packs
            .iter()
            .any(|pack| pack.id == "valid-private-eval"));
        assert!(scan.diagnostics.iter().any(|diagnostic| {
            diagnostic.status == "error" && diagnostic.id.as_deref() == Some("broken-private-eval")
        }));

        let discovered = find_benchmark_pack_path("valid-private-eval", &roots)
            .expect("valid pack should resolve despite unrelated invalid pack");
        assert!(discovered.path.ends_with("pack.yaml"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn benchmark_pack_task_paths_cannot_escape_pack_dir() {
        let root =
            std::env::temp_dir().join(format!("benchforge-pack-escape-{}", uuid::Uuid::new_v4()));
        let pack_dir = root.join("escape-eval");
        fs::create_dir_all(&pack_dir).expect("pack dir should create");
        fs::write(
            pack_dir.join("pack.yaml"),
            r#"
id: escape-eval
name: Escape Eval
version: 0.1.0
tasks:
  - ../outside.yaml
"#,
        )
        .expect("pack should write");
        fs::write(root.join("outside.yaml"), "id: nope\n").expect("outside task should write");
        let pack = load_pack_from_path_with_source(&pack_dir.join("pack.yaml"), "user")
            .expect("pack loads");
        let err = load_tasks(&pack).expect_err("task escape should fail");
        assert!(err.contains("task path must stay inside the benchmark pack"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn test_summary_parses_unittest_and_node_test() {
        let unittest = parse_test_summary("Ran 3 tests in 0.001s\n\nOK", "", Some("unittest"));
        assert_eq!(unittest["passed"], serde_json::json!(true));
        let node = parse_test_summary(
            "# tests 3\n# suites 0\n# pass 3\n# fail 0\n# duration_ms 12",
            "",
            Some("node-test"),
        );
        assert_eq!(node["passed"], serde_json::json!(true));
    }

    #[test]
    fn docker_scoring_is_limited_to_python_non_prompt_tasks() {
        let python_repo = test_task("python", "repo_patch", Some("python"));
        let js_repo = test_task("javascript", "repo_patch", Some("javascript"));
        let python_prompt = test_task("prompt", "prompt", Some("python"));

        assert!(task_supports_docker_scoring(&python_repo));
        assert!(uses_docker_scoring(true, &python_repo));
        assert!(!uses_docker_scoring(false, &python_repo));
        assert!(!task_supports_docker_scoring(&js_repo));
        assert!(!task_supports_docker_scoring(&python_prompt));
    }

    #[test]
    fn docker_scoring_run_args_disable_network_and_cap_resources() {
        let args = docker_scoring_run_args(
            "benchforge-test",
            Path::new("/tmp/benchforge-workspace"),
            Path::new("/tmp/benchforge-artifacts"),
            "benchforge-runner:local",
            &["python".into(), "-m".into(), "unittest".into()],
        );

        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--network" && pair[1] == "none"));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--cpus" && pair[1] == DOCKER_SCORING_CPUS));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--memory" && pair[1] == DOCKER_SCORING_MEMORY));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--pids-limit" && pair[1] == DOCKER_SCORING_PIDS_LIMIT));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--cap-drop" && pair[1] == DOCKER_SCORING_CAP_DROP));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--security-opt" && pair[1] == DOCKER_SCORING_SECURITY_OPT));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--name" && pair[1] == "benchforge-test"));
        assert!(args.contains(&"/workspace".to_string()));
        let limits = docker_scoring_resource_limits();
        assert_eq!(limits["cap_drop"][0], DOCKER_SCORING_CAP_DROP);
        assert_eq!(limits["security_opt"][0], DOCKER_SCORING_SECURITY_OPT);
        assert!(args.ends_with(&[
            "python".to_string(),
            "-m".to_string(),
            "unittest".to_string()
        ]));
    }

    #[test]
    fn docker_scoring_version_args_disable_network_and_cap_resources() {
        let args = docker_scoring_version_run_args(
            "benchforge-version-test",
            "benchforge-runner:local",
            &["python".into(), "--version".into()],
        );

        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--network" && pair[1] == "none"));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--cpus" && pair[1] == DOCKER_SCORING_CPUS));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--memory" && pair[1] == DOCKER_SCORING_MEMORY));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--pids-limit" && pair[1] == DOCKER_SCORING_PIDS_LIMIT));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--cap-drop" && pair[1] == DOCKER_SCORING_CAP_DROP));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--security-opt" && pair[1] == DOCKER_SCORING_SECURITY_OPT));
        assert!(args.ends_with(&[
            "benchforge-runner:local".to_string(),
            "python".to_string(),
            "--version".to_string()
        ]));
    }

    #[test]
    fn docker_image_metadata_parses_inspect_identity() {
        let metadata = docker_scoring_image_metadata_from_inspect(
            "benchforge-runner:local",
            Some("dockerfile-sha".into()),
            r#"[{
                "Id": "sha256:abc123",
                "RepoDigests": ["benchforge-runner@sha256:def456", "<none>@<none>"]
            }]"#,
        )
        .expect("docker inspect metadata should parse");

        assert_eq!(metadata.image, "benchforge-runner:local");
        assert_eq!(metadata.image_id.as_deref(), Some("sha256:abc123"));
        assert_eq!(
            metadata.image_digest.as_deref(),
            Some("benchforge-runner@sha256:def456")
        );
        assert_eq!(
            metadata.repo_digests,
            vec!["benchforge-runner@sha256:def456".to_string()]
        );
        assert_eq!(
            metadata.dockerfile_sha256.as_deref(),
            Some("dockerfile-sha")
        );
    }

    #[test]
    fn docker_preflight_skips_without_eligible_tasks() {
        let tasks = vec![
            test_task("llm", "prompt", None),
            test_task("javascript", "repo_patch", Some("javascript")),
        ];

        assert!(validate_docker_scoring_preflight(&tasks, true, &|| false).is_ok());
    }

    #[test]
    fn prompt_scoring_checks_exact_contains_and_json_fields() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec!["local".into(), "cloud".into()],
            expect_regex: vec![],
            expect_not_contains: vec!["offline-only".into()],
            expect_json: true,
            json_field_equals: HashMap::from([
                ("task".into(), serde_json::json!("benchmark")),
                ("valid".into(), serde_json::json!(true)),
            ]),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };
        let score = score_prompt_response(
            &scoring,
            r#"```json
{"task":"benchmark","valid":true,"items":["local","cloud"]}
```"#,
        );
        assert_eq!(score.status, "passed");
        assert_eq!(score.score, 1.0);
    }

    #[test]
    fn prompt_scoring_reports_partial_failure() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: Some("benchforge".into()),
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: false,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };
        let score = score_prompt_response(&scoring, "BenchForge!");
        assert_eq!(score.status, "failed");
        assert!(score.score < 1.0);
        assert_eq!(
            score.error_message.as_deref(),
            Some("prompt expectations failed: exact")
        );
    }

    #[test]
    fn prompt_failure_message_lists_failed_checks() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: true,
            json_field_equals: HashMap::from([("task".into(), serde_json::json!("benchmark"))]),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::from([(
                "evidence_ids".into(),
                vec![serde_json::json!("A"), serde_json::json!("B")],
            )]),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };
        let score = score_prompt_response(&scoring, "not json");
        assert_eq!(score.status, "failed");
        assert_eq!(
            score.error_message.as_deref(),
            Some(
                "prompt expectations failed: json_valid, json_field_equals(task), json_field_array_exact(evidence_ids)"
            )
        );
    }

    #[test]
    fn prompt_failure_error_code_classifies_structured_failures() {
        let structured_scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: true,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };
        let structured_score = score_prompt_response(&structured_scoring, "not json");
        assert_eq!(
            prompt_failure_error_code(&structured_score.tests),
            Some("invalid_output_format")
        );

        let text_scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: Some("OK".into()),
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: false,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };
        let text_score = score_prompt_response(&text_scoring, "NO");
        assert_eq!(
            prompt_failure_error_code(&text_score.tests),
            Some("test_failed")
        );
    }

    #[test]
    fn failed_prompt_scoring_persists_invalid_output_format_error_code() {
        let root = std::env::temp_dir().join(format!(
            "benchforge-invalid-output-format-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("temp root should be created");
        let task_source = root.join("task.yaml");
        fs::write(&task_source, "id: invalid-output-format\n")
            .expect("task source should be written");
        let mut task = test_task("invalid-output-format", "prompt", None);
        task.prompt = "Return compact JSON.".into();
        task.source_path = task_source;
        task.scoring.expect_json = true;
        let pack = test_pack("llm-basics");
        let target = test_target("mock-agent", "mock");
        let conn = store::open_memory().expect("db should open");

        let result = run_prompt_task(&conn, &target, &pack, &task, 0, 1, None, &|| false)
            .expect("failed scoring should persist a run result");
        assert_eq!(result.status, "failed");

        let record = store::list_results(&conn)
            .expect("results should list")
            .into_iter()
            .find(|record| record.id == result.id)
            .expect("failed result should persist");
        assert!(record.setup_time_ms.is_some());
        assert!(record.target_time_ms.is_some());
        assert_eq!(record.error_code.as_deref(), Some("invalid_output_format"));
        assert!(record
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("json_valid")));

        let artifacts =
            store::list_artifacts(&conn, Some(&result.id)).expect("artifacts should list");
        let result_artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == "result_json")
            .expect("result artifact should persist");
        let result_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&result_artifact.path).unwrap()).unwrap();
        assert_eq!(
            result_json.get("error_code"),
            Some(&serde_json::json!("invalid_output_format"))
        );
        assert_required_v1_metrics_are_explicit(&result_json["metrics"]);
        assert!(result_json["metrics"]["setup_time_ms"].is_number());
        assert!(result_json["metrics"]["target_time_ms"].is_number());
        assert!(result_json["metrics"]["exit_code"].is_null());
        assert!(result_json["metrics"]["files_changed"].is_null());
        assert!(result_json
            .get("error_message")
            .and_then(|value| value.as_str())
            .is_some_and(|message| message.contains("json_valid")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prompt_scoring_supports_regex_contains_and_numeric_tolerance() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec![],
            expect_regex: vec![r"(?i)model\s+b".into()],
            expect_not_contains: vec![],
            expect_json: true,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::from([(
                "supported_claim_ids".into(),
                vec!["claim_1".into(), "claim_3".into()],
            )]),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::from([(
                "total_cost".into(),
                JsonNumberCloseSpec {
                    expected: 0.04395,
                    tolerance: 0.00001,
                },
            )]),
            json_field_number_bounds: HashMap::new(),
        };
        let score = score_prompt_response(
            &scoring,
            r#"{"recommendation":"Choose Model B","supported_claim_ids":["claim_1","claim_3"],"total_cost":0.043951}"#,
        );
        assert_eq!(score.status, "passed");
        assert_eq!(score.score, 1.0);
    }

    #[test]
    fn prompt_scoring_requires_exact_json_array_membership() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: true,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::from([(
                "evidence_ids".into(),
                vec![
                    serde_json::json!("A3"),
                    serde_json::json!("A6"),
                    serde_json::json!("A8"),
                ],
            )]),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };

        let reordered = score_prompt_response(&scoring, r#"{"evidence_ids":["A8","A3","A6"]}"#);
        assert_eq!(reordered.status, "passed");

        let extra = score_prompt_response(&scoring, r#"{"evidence_ids":["A3","A6","A8","A9"]}"#);
        assert_eq!(extra.status, "failed");
        assert!(extra
            .tests
            .pointer("/checks/1/unexpected/0")
            .is_some_and(|value| value == "A9"));
    }

    #[test]
    fn prompt_scoring_requires_ordered_json_array_membership() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: true,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::from([(
                "impacted_products".into(),
                vec![
                    serde_json::json!("Billing API"),
                    serde_json::json!("Checkout API"),
                ],
            )]),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };

        let ordered = score_prompt_response(
            &scoring,
            r#"{"impacted_products":["Billing API","Checkout API"]}"#,
        );
        assert_eq!(ordered.status, "passed");

        let reversed = score_prompt_response(
            &scoring,
            r#"{"impacted_products":["Checkout API","Billing API"]}"#,
        );
        assert_eq!(reversed.status, "failed");
        assert_eq!(
            reversed.tests.pointer("/checks/1/first_mismatch_index"),
            Some(&serde_json::json!(0))
        );
    }

    #[test]
    fn prompt_scoring_requires_exact_json_object_keys() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: true,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::from([(
                "$".into(),
                vec![
                    "account_name".into(),
                    "incident_type".into(),
                    "severity".into(),
                ],
            )]),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::new(),
        };

        let exact = score_prompt_response(
            &scoring,
            r#"{"severity":"P1","incident_type":"checkout_failure","account_name":"Helios Bank"}"#,
        );
        assert_eq!(exact.status, "passed");

        let extra = score_prompt_response(
            &scoring,
            r#"{"account_name":"Helios Bank","incident_type":"checkout_failure","severity":"P1","confidence":"high"}"#,
        );
        assert_eq!(extra.status, "failed");
        assert!(extra
            .tests
            .pointer("/checks/1/unexpected/0")
            .is_some_and(|value| value == "confidence"));
    }

    #[test]
    fn prompt_scoring_checks_json_number_bounds() {
        let scoring = ScoringSpec {
            command: vec![],
            parse: None,
            expect_exact: None,
            expect_contains: vec![],
            expect_regex: vec![],
            expect_not_contains: vec![],
            expect_json: true,
            json_field_equals: HashMap::new(),
            json_field_contains: HashMap::new(),
            json_field_object_keys_exact: HashMap::new(),
            json_field_array_exact: HashMap::new(),
            json_field_array_exact_ordered: HashMap::new(),
            json_field_number_close: HashMap::new(),
            json_field_number_bounds: HashMap::from([(
                "monthly_cost_usd".into(),
                JsonNumberBoundsSpec {
                    min: Some(0.0),
                    max: Some(80.0),
                },
            )]),
        };

        let inside = score_prompt_response(&scoring, r#"{"monthly_cost_usd":50}"#);
        assert_eq!(inside.status, "passed");

        let over_budget = score_prompt_response(&scoring, r#"{"monthly_cost_usd":80.01}"#);
        assert_eq!(over_budget.status, "failed");
        assert_eq!(
            over_budget.tests.pointer("/checks/1/max_passed"),
            Some(&serde_json::json!(false))
        );
    }

    #[test]
    fn estimates_cost_from_token_usage_and_target_pricing() {
        let config = serde_json::json!({
            "input_price_usd_per_million_tokens": 2.0,
            "output_price_usd_per_million_tokens": 10.0
        });
        let metrics = serde_json::Map::from_iter([
            ("prompt_tokens".into(), serde_json::json!(1_000)),
            ("completion_tokens".into(), serde_json::json!(500)),
        ]);
        assert_eq!(
            estimate_cost_usd("direct_model", "openai-compatible", &config, &metrics),
            Some(0.007)
        );
    }

    #[test]
    fn estimates_cost_from_cache_usage_and_cache_pricing() {
        let config = serde_json::json!({
            "input_price_usd_per_million_tokens": 2.0,
            "output_price_usd_per_million_tokens": 10.0,
            "cache_read_price_usd_per_million_tokens": 0.5,
            "cache_write_price_usd_per_million_tokens": 3.0
        });
        let metrics = serde_json::Map::from_iter([
            ("prompt_tokens".into(), serde_json::json!(1_000)),
            ("completion_tokens".into(), serde_json::json!(500)),
            ("cache_read_tokens".into(), serde_json::json!(200)),
            ("cache_write_tokens".into(), serde_json::json!(100)),
        ]);
        assert_eq!(
            estimate_cost_usd("direct_model", "openai-compatible", &config, &metrics),
            Some(0.0068)
        );
    }

    #[test]
    fn estimates_anthropic_cache_usage_when_input_tokens_exclude_cache() {
        let config = serde_json::json!({
            "input_price_usd_per_million_tokens": 2.0,
            "output_price_usd_per_million_tokens": 10.0,
            "cache_read_price_usd_per_million_tokens": 0.5,
            "cache_write_price_usd_per_million_tokens": 3.0
        });
        let metrics = serde_json::Map::from_iter([
            ("prompt_tokens".into(), serde_json::json!(700)),
            ("completion_tokens".into(), serde_json::json!(500)),
            ("cache_read_tokens".into(), serde_json::json!(200)),
            ("cache_write_tokens".into(), serde_json::json!(100)),
        ]);
        assert_eq!(
            estimate_cost_usd("direct_model", "anthropic", &config, &metrics),
            Some(0.0068)
        );
    }

    #[test]
    fn reports_cache_pricing_fallback_assumptions() {
        let config = serde_json::json!({
            "input_price_usd_per_million_tokens": 2.0,
            "output_price_usd_per_million_tokens": 10.0
        });
        let metrics = serde_json::Map::from_iter([
            ("prompt_tokens".into(), serde_json::json!(1_000)),
            ("completion_tokens".into(), serde_json::json!(500)),
            ("cache_read_tokens".into(), serde_json::json!(200)),
            ("cache_write_tokens".into(), serde_json::json!(100)),
        ]);

        assert_eq!(
            cache_pricing_fallback_assumptions(&config, &metrics),
            vec![CACHE_READ_PRICED_AS_INPUT, CACHE_WRITE_PRICED_AS_INPUT]
        );

        let mut result_metrics = serde_json::Map::new();
        insert_pricing_assumption_metrics(
            &mut result_metrics,
            &cache_pricing_fallback_assumptions(&config, &metrics),
        );

        assert_eq!(
            result_metrics["cache_read_priced_with_input_price"],
            serde_json::json!(1.0)
        );
        assert_eq!(
            result_metrics["cache_write_priced_with_input_price"],
            serde_json::json!(1.0)
        );
        assert_eq!(
            result_metrics["pricing_assumption"],
            serde_json::json!(
                "cache_read_tokens_priced_as_input;cache_write_tokens_priced_as_input"
            )
        );
    }

    #[test]
    fn omits_cache_pricing_fallback_when_cache_prices_exist() {
        let config = serde_json::json!({
            "input_price_usd_per_million_tokens": 2.0,
            "output_price_usd_per_million_tokens": 10.0,
            "cache_read_price_usd_per_million_tokens": 0.5,
            "cache_creation_price_usd_per_million_tokens": 3.0
        });
        let metrics = serde_json::Map::from_iter([
            ("prompt_tokens".into(), serde_json::json!(1_000)),
            ("completion_tokens".into(), serde_json::json!(500)),
            ("cached_tokens".into(), serde_json::json!(200)),
            ("cache_write_tokens".into(), serde_json::json!(100)),
        ]);

        assert!(cache_pricing_fallback_assumptions(&config, &metrics).is_empty());
    }

    #[test]
    fn estimates_unpriced_local_cost_as_zero_without_usage_metrics() {
        let config = serde_json::json!({
            "model": "local",
            "base_url": "http://127.0.0.1:8080/v1"
        });
        let metrics = serde_json::Map::new();

        assert_eq!(
            estimate_cost_usd("direct_model", "openai-compatible", &config, &metrics),
            Some(0.0)
        );
    }

    #[test]
    fn leaves_unpriced_remote_cost_unknown() {
        let config = serde_json::json!({
            "model": "remote",
            "base_url": "https://api.example.com/v1"
        });
        let metrics = serde_json::Map::from_iter([
            ("prompt_tokens".into(), serde_json::json!(1_000)),
            ("completion_tokens".into(), serde_json::json!(500)),
        ]);

        assert_eq!(
            estimate_cost_usd("direct_model", "openai-compatible", &config, &metrics),
            None
        );
    }

    #[test]
    fn estimates_output_tokens_per_second_from_completion_tokens() {
        let metrics =
            serde_json::Map::from_iter([("completion_tokens".into(), serde_json::json!(50))]);
        assert_eq!(
            estimate_output_tokens_per_second(&metrics, 2_000),
            Some(25.0)
        );
        assert_eq!(estimate_output_tokens_per_second(&metrics, 0), None);
        assert_eq!(
            estimate_output_tokens_per_second(&serde_json::Map::new(), 2_000),
            None
        );
    }

    #[test]
    fn generation_settings_capture_effective_defaults() {
        let config = serde_json::json!({
            "temperature": 0.2,
            "top_p": 0.9,
            "max_tokens": 128,
            "seed": 42,
            "timeout_seconds": 5_000,
            "retry_count": 7
        });
        assert_eq!(
            generation_settings(&config, 512),
            serde_json::json!({
                "temperature": 0.2,
                "top_p": 0.9,
                "max_tokens": 128,
                "seed": 42,
                "timeout_seconds": 3600,
                "retry_count": 5
            })
        );

        let invalid = serde_json::json!({
            "temperature": -1.0,
            "top_p": 2.0,
            "max_tokens": 0,
            "timeout_seconds": 0,
            "retry_count": 9
        });
        assert_eq!(
            generation_settings(&invalid, 512),
            serde_json::json!({
                "temperature": 0.0,
                "top_p": 1.0,
                "max_tokens": 512,
                "timeout_seconds": 120,
                "retry_count": 5
            })
        );
    }

    #[test]
    fn openai_responses_payload_uses_responses_shape() {
        let config = serde_json::json!({
            "temperature": 0.2,
            "top_p": 0.9,
            "max_tokens": 128,
            "seed": 42
        });
        let payload =
            openai_responses_payload("gpt-5-mini", "Answer directly.", "Say hello.", &config, 512);
        assert_eq!(payload["model"], "gpt-5-mini");
        assert_eq!(payload["instructions"], "Answer directly.");
        assert_eq!(payload["input"][0]["content"], "Say hello.");
        assert_eq!(payload["max_output_tokens"], 128);
        assert_eq!(payload["store"], false);
        assert!(payload.get("messages").is_none());
        assert!(payload.get("max_tokens").is_none());
        assert!(payload.get("seed").is_none());
    }

    #[test]
    fn openai_chat_metrics_accept_provider_usage_variants() {
        let response = serde_json::json!({
            "model": "gpt-chat",
            "choices": [{
                "message": {"role": "assistant", "content": "OK"},
                "finish_reason": "stop"
            }],
            "usage": {
                "input_tokens": 9,
                "output_tokens": 4,
                "input_tokens_details": {"cached_tokens": 3},
                "completion_tokens_details": {"reasoning_tokens": 2}
            }
        });

        let metrics = openai_response_metrics(&response);

        assert_eq!(
            metrics.get("provider_model"),
            Some(&serde_json::json!("gpt-chat"))
        );
        assert_eq!(
            metrics.get("finish_reason"),
            Some(&serde_json::json!("stop"))
        );
        assert_eq!(metrics.get("prompt_tokens"), Some(&serde_json::json!(9)));
        assert_eq!(
            metrics.get("completion_tokens"),
            Some(&serde_json::json!(4))
        );
        assert_eq!(metrics.get("total_tokens"), Some(&serde_json::json!(13)));
        assert_eq!(metrics.get("reasoning_tokens"), Some(&serde_json::json!(2)));
        assert_eq!(metrics.get("cached_tokens"), Some(&serde_json::json!(3)));
        assert_eq!(
            metrics.get("cache_read_tokens"),
            Some(&serde_json::json!(3))
        );
    }

    #[test]
    fn openai_responses_text_and_metrics_map_to_benchforge_fields() {
        let response = serde_json::json!({
            "model": "gpt-5-mini",
            "status": "completed",
            "output": [
                {
                    "type": "message",
                    "content": [
                        {"type": "output_text", "text": "Hello"},
                        {"type": "output_text", "text": "world"}
                    ]
                }
            ],
            "usage": {
                "input_tokens": 11,
                "output_tokens": 7,
                "total_tokens": 18,
                "input_tokens_details": {"cached_tokens": 4},
                "output_tokens_details": {"reasoning_tokens": 3}
            }
        });
        assert_eq!(
            openai_responses_text(&response).as_deref(),
            Some("Hello\nworld")
        );
        let metrics = openai_responses_metrics(&response);
        assert_eq!(
            metrics.get("provider_model"),
            Some(&serde_json::json!("gpt-5-mini"))
        );
        assert_eq!(
            metrics.get("finish_reason"),
            Some(&serde_json::json!("completed"))
        );
        assert_eq!(metrics.get("prompt_tokens"), Some(&serde_json::json!(11)));
        assert_eq!(
            metrics.get("completion_tokens"),
            Some(&serde_json::json!(7))
        );
        assert_eq!(metrics.get("total_tokens"), Some(&serde_json::json!(18)));
        assert_eq!(metrics.get("reasoning_tokens"), Some(&serde_json::json!(3)));
        assert_eq!(metrics.get("cached_tokens"), Some(&serde_json::json!(4)));
        assert_eq!(
            metrics.get("cache_read_tokens"),
            Some(&serde_json::json!(4))
        );
    }

    #[test]
    fn openai_chat_stream_accumulates_content_usage_and_ttft() {
        let mut state = StreamParseState::new(StreamFormat::OpenAiChat);
        state.handle_stdout_line(
            "data: {\"model\":\"gpt-test\",\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n",
            80.0,
        );
        state.handle_stdout_line("\n", 81.0);
        state.handle_stdout_line(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"finish_reason\":null}]}\n",
            123.0,
        );
        state.handle_stdout_line("\n", 124.0);
        state.handle_stdout_line(
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"prompt_tokens_details\":{\"cached_tokens\":6},\"completion_tokens_details\":{\"reasoning_tokens\":1}}}\n",
            140.0,
        );
        state.handle_stdout_line("\n", 141.0);
        state.handle_stdout_line("__BENCHFORGE_HTTP_STATUS__:200\n", 150.0);
        state.handle_stdout_line("__BENCHFORGE_TIME_STARTTRANSFER__:0.050000\n", 151.0);
        state.handle_stdout_line("__BENCHFORGE_TIME_TOTAL__:0.160000\n", 152.0);
        let (response, error) = state.finish(160.0);
        assert!(error.is_none());
        assert_eq!(response.content, "Hello");
        assert_eq!(response.http_status, Some(200));
        assert_eq!(response.time_to_first_byte_ms, Some(50.0));
        assert_eq!(response.time_to_first_token_ms, Some(123.0));
        assert_eq!(response.request_total_ms, Some(160.0));
        assert_eq!(
            response.metrics.get("provider_model"),
            Some(&serde_json::json!("gpt-test"))
        );
        assert_eq!(
            response.metrics.get("completion_tokens"),
            Some(&serde_json::json!(2))
        );
        assert_eq!(
            response.metrics.get("total_tokens"),
            Some(&serde_json::json!(12))
        );
        assert_eq!(
            response.metrics.get("reasoning_tokens"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            response.metrics.get("cached_tokens"),
            Some(&serde_json::json!(6))
        );
        assert_eq!(
            response.metrics.get("cache_read_tokens"),
            Some(&serde_json::json!(6))
        );
        assert_eq!(
            response.metrics.get("finish_reason"),
            Some(&serde_json::json!("stop"))
        );
    }

    #[test]
    fn anthropic_metrics_capture_cache_usage() {
        let response = serde_json::json!({
            "model": "claude-cache",
            "stop_reason": "end_turn",
            "content": [{"type": "text", "text": "OK"}],
            "usage": {
                "input_tokens": 12,
                "output_tokens": 3,
                "cache_read_input_tokens": 5,
                "cache_creation_input_tokens": 2
            }
        });

        let metrics = anthropic_response_metrics(&response);

        assert_eq!(
            metrics.get("provider_model"),
            Some(&serde_json::json!("claude-cache"))
        );
        assert_eq!(metrics.get("prompt_tokens"), Some(&serde_json::json!(12)));
        assert_eq!(
            metrics.get("completion_tokens"),
            Some(&serde_json::json!(3))
        );
        assert_eq!(metrics.get("total_tokens"), Some(&serde_json::json!(15)));
        assert_eq!(metrics.get("cached_tokens"), Some(&serde_json::json!(5)));
        assert_eq!(
            metrics.get("cache_read_tokens"),
            Some(&serde_json::json!(5))
        );
        assert_eq!(
            metrics.get("cache_write_tokens"),
            Some(&serde_json::json!(2))
        );
    }

    #[test]
    fn openai_responses_stream_accumulates_delta_and_completed_metrics() {
        let mut state = StreamParseState::new(StreamFormat::OpenAiResponses);
        state.handle_stdout_line("event: response.output_text.delta\n", 90.0);
        state.handle_stdout_line(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n",
            91.0,
        );
        state.handle_stdout_line("\n", 92.0);
        state.handle_stdout_line("event: response.completed\n", 150.0);
        state.handle_stdout_line(
            "data: {\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-5-mini\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":5,\"output_tokens\":1,\"total_tokens\":6,\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n",
            151.0,
        );
        state.handle_stdout_line("\n", 152.0);
        let (response, error) = state.finish(160.0);
        assert!(error.is_none());
        assert_eq!(response.content, "OK");
        assert_eq!(response.time_to_first_token_ms, Some(91.0));
        assert_eq!(
            response.metrics.get("provider_model"),
            Some(&serde_json::json!("gpt-5-mini"))
        );
        assert_eq!(
            response.metrics.get("prompt_tokens"),
            Some(&serde_json::json!(5))
        );
        assert_eq!(
            response.metrics.get("finish_reason"),
            Some(&serde_json::json!("completed"))
        );
    }

    #[test]
    fn anthropic_stream_accumulates_text_usage_and_ttft() {
        let mut state = StreamParseState::new(StreamFormat::AnthropicMessages);
        state.handle_stdout_line("event: message_start\n", 70.0);
        state.handle_stdout_line(
            "data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-test\",\"usage\":{\"input_tokens\":8}}}\n",
            71.0,
        );
        state.handle_stdout_line("\n", 72.0);
        state.handle_stdout_line("event: content_block_delta\n", 130.0);
        state.handle_stdout_line(
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n",
            131.0,
        );
        state.handle_stdout_line("\n", 132.0);
        state.handle_stdout_line("event: message_delta\n", 180.0);
        state.handle_stdout_line(
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n",
            181.0,
        );
        state.handle_stdout_line("\n", 182.0);
        let (response, error) = state.finish(190.0);
        assert!(error.is_none());
        assert_eq!(response.content, "Hi");
        assert_eq!(response.time_to_first_token_ms, Some(131.0));
        assert_eq!(
            response.metrics.get("provider_model"),
            Some(&serde_json::json!("claude-test"))
        );
        assert_eq!(
            response.metrics.get("prompt_tokens"),
            Some(&serde_json::json!(8))
        );
        assert_eq!(
            response.metrics.get("completion_tokens"),
            Some(&serde_json::json!(2))
        );
        assert_eq!(
            response.metrics.get("total_tokens"),
            Some(&serde_json::json!(10))
        );
        assert_eq!(
            response.metrics.get("finish_reason"),
            Some(&serde_json::json!("end_turn"))
        );
    }

    #[test]
    fn cloud_contract_streaming_responses_use_provider_sse_shapes() {
        let body = r#"{"stream":true}"#;
        let attempts = cloud_contract_test_attempts();
        let chat = cloud_contract_response("/v1/chat/completions", body, &attempts);
        assert_eq!(chat.status, 200);
        assert_eq!(chat.content_type, "text/event-stream");
        assert!(chat.streaming);
        assert!(chat.body.contains("benchforge-"));
        assert!(chat.body.contains("cloud-ok"));
        assert!(chat.body.contains("data: [DONE]"));

        let responses = cloud_contract_response("/v1/responses", body, &attempts);
        assert_eq!(responses.content_type, "text/event-stream");
        assert!(responses.body.contains("event: response.output_text.delta"));
        assert!(responses.body.contains("event: response.completed"));
        assert!(responses.body.contains("contract-openai-responses-stream"));

        let anthropic = cloud_contract_response("/v1/messages", body, &attempts);
        assert_eq!(anthropic.content_type, "text/event-stream");
        assert!(anthropic.body.contains("event: message_start"));
        assert!(anthropic.body.contains("event: content_block_delta"));
        assert!(anthropic.body.contains("contract-anthropic-stream"));
    }

    #[test]
    fn cloud_contract_code_edit_response_returns_edit_protocol() {
        let body = r#"{"model":"contract-code-edit"}"#;
        let attempts = cloud_contract_test_attempts();
        let response = cloud_contract_response("/v1/chat/completions", body, &attempts);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json");
        let json: serde_json::Value =
            serde_json::from_str(&response.body).expect("response body should be JSON");
        let content = json
            .pointer("/choices/0/message/content")
            .and_then(|value| value.as_str())
            .expect("response should include assistant content");
        let edits: serde_json::Value =
            serde_json::from_str(content).expect("assistant content should be edit JSON");
        assert_eq!(
            edits
                .pointer("/edits/0/path")
                .and_then(|value| value.as_str()),
            Some("config_merge.py")
        );
        assert!(content.contains("merge_config"));
    }

    #[test]
    fn cloud_contract_code_edit_stream_response_returns_edit_protocol() {
        let body = r#"{"model":"contract-code-edit","stream":true}"#;
        let attempts = cloud_contract_test_attempts();
        let response = cloud_contract_response("/v1/chat/completions", body, &attempts);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/event-stream");
        assert!(response.streaming);
        assert!(response.body.contains("contract-code-edit"));
        assert!(response.body.contains("config_merge.py"));
        assert!(response.body.contains("data: [DONE]"));
    }

    #[test]
    fn cloud_contract_transient_rate_limit_recovers_after_first_attempt() {
        let body = r#"{"model":"contract-transient-rate-limit"}"#;
        let attempts = cloud_contract_test_attempts();
        let first = cloud_contract_response("/v1/chat/completions", body, &attempts);
        assert_eq!(first.status, 429);
        assert!(first
            .extra_headers
            .iter()
            .any(|(name, value)| { name.eq_ignore_ascii_case("Retry-After") && value == "0" }));

        let second = cloud_contract_response("/v1/chat/completions", body, &attempts);
        assert_eq!(second.status, 200);
        assert!(second.body.contains(CLOUD_CONTRACT_EXPECTED_REPLY));
    }

    #[test]
    fn cloud_contract_streaming_transient_rate_limit_recovers_after_first_attempt() {
        let body = r#"{"model":"contract-transient-rate-limit-stream","stream":true}"#;
        let attempts = cloud_contract_test_attempts();
        let first = cloud_contract_response("/v1/chat/completions", body, &attempts);
        assert_eq!(first.status, 429);
        assert!(first
            .extra_headers
            .iter()
            .any(|(name, value)| { name.eq_ignore_ascii_case("Retry-After") && value == "0" }));

        let second = cloud_contract_response("/v1/chat/completions", body, &attempts);
        assert_eq!(second.status, 200);
        assert_eq!(second.content_type, "text/event-stream");
        assert!(second.streaming);
        assert!(second.body.contains("benchforge-"));
        assert!(second.body.contains("cloud-ok"));
        assert!(second.body.contains("data: [DONE]"));
    }

    #[test]
    fn worker_error_mapping_preserves_harness_codes() {
        let event = serde_json::json!({
            "status": "error",
            "error_code": "configuration_missing",
            "error_message": "set harness.command"
        });
        let code = worker_run_error_code("error", &event, None);
        assert_eq!(code.as_deref(), Some("configuration_missing"));
        assert_eq!(
            worker_run_error_message("error", &event, None, code.as_deref()).as_deref(),
            Some("set harness.command")
        );

        let benchmark_event = serde_json::json!({"status": "failed"});
        assert_eq!(
            worker_run_error_code("failed", &benchmark_event, Some(0.0)).as_deref(),
            Some("benchmark_failed")
        );

        let security_event = serde_json::json!({"status": "failed"});
        let code = worker_run_error_code("failed", &security_event, Some(2.0));
        assert_eq!(code.as_deref(), Some("security_findings"));
        assert_eq!(
            worker_run_error_message("failed", &security_event, Some(2.0), code.as_deref())
                .as_deref(),
            Some("2 security finding(s) detected")
        );
    }

    #[test]
    fn run_request_defaults_warmup_to_zero() {
        let request: RunQuickSmokeRequest = serde_json::from_value(serde_json::json!({
            "targetIds": ["mock-agent"],
            "benchmarkPackId": "llm-core"
        }))
        .expect("request should deserialize");
        assert_eq!(request.repetitions, 1);
        assert_eq!(request.warmup_runs, 0);
        assert_eq!(request.concurrency, 1);
        assert_eq!(request.max_cost_usd, None);
        assert!(request.task_ids.is_empty());

        let request: RunQuickSmokeRequest = serde_json::from_value(serde_json::json!({
            "targetIds": ["mock-agent"],
            "benchmarkPackId": "llm-core",
            "taskIds": ["llm-core-classification-001"],
            "warmupRuns": 2,
            "concurrency": 4,
            "maxCostUsd": 0.25
        }))
        .expect("request should deserialize");
        assert_eq!(
            request.task_ids,
            vec!["llm-core-classification-001".to_string()]
        );
        assert_eq!(request.warmup_runs, 2);
        assert_eq!(request.concurrency, 4);
        assert_eq!(request.max_cost_usd, Some(0.25));
    }

    #[test]
    fn run_task_filter_preserves_pack_order_and_rejects_bad_ids() {
        let pack = load_pack("llm-core").expect("pack should load");
        let tasks = load_tasks(&pack).expect("tasks should load");
        let first = tasks[0].id.clone();
        let second = tasks[1].id.clone();

        let selected = select_tasks_for_run(tasks.clone(), &[second.clone(), first.clone()])
            .expect("task subset should filter");
        assert_eq!(
            selected
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec![first.as_str(), second.as_str()]
        );

        let duplicate_err = select_tasks_for_run(tasks.clone(), &[first.clone(), first.clone()])
            .expect_err("duplicate task IDs should fail");
        assert!(duplicate_err.starts_with("task_filter_invalid"));

        let missing_err = select_tasks_for_run(tasks, &["missing-task".into()])
            .expect_err("missing task IDs should fail");
        assert!(missing_err.contains("missing-task"));
    }

    #[test]
    fn warmup_skips_non_model_targets_and_runs_mock() {
        let cli_target = store::TargetRecord {
            id: "cli".into(),
            name: "CLI".into(),
            kind: "cli_agent".into(),
            adapter_id: "codex-cli".into(),
            config_json: "{}".into(),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        };
        run_target_warmup(&cli_target, &|| false).expect("CLI warmup should be skipped");

        let mock_target = store::TargetRecord {
            id: "mock-agent".into(),
            name: "Mock Agent".into(),
            kind: "mock".into(),
            adapter_id: "mock".into(),
            config_json: "{}".into(),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        };
        run_target_warmup(&mock_target, &|| false).expect("mock warmup should pass");
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
                &serde_json::json!({"azure_api_version": "2025-04-01-preview"})
            ),
            "https://example.openai.azure.com/openai/deployments/deployment/chat/completions?api-version=2025-04-01-preview"
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
    fn target_reproducibility_redacts_and_fingerprints_local_model() {
        let dir = std::env::temp_dir().join(format!("benchforge-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("test dir should be created");
        let file_name = "Tiny-UD-Q4_K_M.gguf";
        let file_path = dir.join(file_name);
        std::fs::write(&file_path, b"tiny model").expect("test gguf should be written");

        let target = store::TargetRecord {
            id: "local".into(),
            name: "Local".into(),
            kind: "direct_model".into(),
            adapter_id: "llama-cpp-openai".into(),
            config_json: "{}".into(),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
        };
        let config = serde_json::json!({
            "model": "local-huggingface",
            "base_url": "http://127.0.0.1:8080/v1",
            "api_key": "secret",
            "api_key_env": "OPENAI_API_KEY",
            "nested": {"token": "secret-token", "prompt_tokens": 123},
            "model_path": dir.to_string_lossy(),
            "gguf_file": file_name,
            "context": 2048,
            "max_tokens": 512,
            "input_price_usd_per_million_tokens": 0.25,
            "output_price_usd_per_million_tokens": 2.0,
            "token_usage_reporting": true
        });
        let metadata = target_reproducibility(&target, &config);

        assert_eq!(
            metadata
                .pointer("/config/api_key")
                .and_then(|value| value.as_str()),
            Some("<redacted>")
        );
        assert_eq!(
            metadata
                .pointer("/config/api_key_env")
                .and_then(|value| value.as_str()),
            Some("<redacted>")
        );
        assert_eq!(
            metadata
                .pointer("/config/nested/token")
                .and_then(|value| value.as_str()),
            Some("<redacted>")
        );
        assert_eq!(
            metadata.pointer("/config/nested/prompt_tokens"),
            Some(&serde_json::json!(123))
        );
        assert_eq!(
            metadata.pointer("/config/max_tokens"),
            Some(&serde_json::json!(512))
        );
        assert_eq!(
            metadata.pointer("/config/input_price_usd_per_million_tokens"),
            Some(&serde_json::json!(0.25))
        );
        assert_eq!(
            metadata.pointer("/config/output_price_usd_per_million_tokens"),
            Some(&serde_json::json!(2.0))
        );
        assert_eq!(
            metadata.pointer("/config/token_usage_reporting"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            metadata
                .pointer("/local_model/file")
                .and_then(|value| value.as_str()),
            Some(file_name)
        );
        assert_eq!(
            metadata
                .pointer("/local_model/quantization")
                .and_then(|value| value.as_str()),
            Some("Q4_K_M")
        );
        assert_eq!(
            metadata
                .pointer("/local_model/sha256")
                .and_then(|value| value.as_str()),
            checksum_file(&file_path).ok().as_deref()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_reproducibility_includes_machine_context() {
        let profile = host_reproducibility();

        assert_eq!(profile["os"], std::env::consts::OS);
        assert_eq!(profile["arch"], std::env::consts::ARCH);
        assert!(profile["hardware"].is_object());
        assert!(
            profile["hardware"]["logical_cores"]
                .as_u64()
                .unwrap_or_default()
                > 0
        );
    }

    #[test]
    fn provider_json_retries_transient_errors() {
        let config = serde_json::json!({"retry_count": 1});
        let mut calls = 0;
        let response = provider_json_with_retry(&config, &|| false, || {
            calls += 1;
            if calls == 1 {
                Err("429 rate_limit_error too many requests".into())
            } else {
                Ok(ProviderHttpResponse {
                    body: r#"{"ok":true}"#.into(),
                    status: Some(200),
                    retry_after_ms: None,
                    time_to_first_byte_ms: Some(12.0),
                    request_total_ms: Some(20.0),
                })
            }
        })
        .expect("transient error should be retried");
        assert_eq!(calls, 2);
        assert_eq!(response.attempts, 2);
        assert_eq!(response.http_status, Some(200));
        assert_eq!(response.retry_after_ms, None);
        assert_eq!(response.retry_delay_ms, Some(250));
        assert_eq!(
            response.json.get("ok").and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn provider_json_honors_retry_after_from_http_response() {
        let config = serde_json::json!({"retry_count": 1});
        let mut calls = 0;
        let response = provider_json_with_retry(&config, &|| false, || {
            calls += 1;
            if calls == 1 {
                Ok(ProviderHttpResponse {
                    body: "Too many requests".into(),
                    status: Some(429),
                    retry_after_ms: Some(0),
                    time_to_first_byte_ms: None,
                    request_total_ms: None,
                })
            } else {
                Ok(ProviderHttpResponse {
                    body: r#"{"ok":true}"#.into(),
                    status: Some(200),
                    retry_after_ms: None,
                    time_to_first_byte_ms: Some(10.0),
                    request_total_ms: Some(15.0),
                })
            }
        })
        .expect("rate limit with Retry-After should be retried");
        assert_eq!(calls, 2);
        assert_eq!(response.attempts, 2);
        assert_eq!(response.retry_after_ms, Some(0));
        assert_eq!(response.retry_delay_ms, Some(0));
    }

    #[test]
    fn provider_json_error_preserves_exhausted_retry_metrics() {
        let config = serde_json::json!({"retry_count": 1});
        let mut calls = 0;
        let error = provider_json_with_retry(&config, &|| false, || {
            calls += 1;
            Ok(ProviderHttpResponse {
                body: "Too many requests".into(),
                status: Some(429),
                retry_after_ms: Some(0),
                time_to_first_byte_ms: None,
                request_total_ms: None,
            })
        })
        .unwrap_err();
        assert_eq!(calls, 2);
        assert!(error.contains("provider_attempts 2"), "{error}");
        assert!(error.contains("provider_retry_delay_ms 0"), "{error}");
        assert!(error.contains("retry_after_ms 0"), "{error}");
        assert!(error.contains("http_status 429"), "{error}");
    }

    #[test]
    fn provider_json_error_preserves_response_timing_metrics() {
        let config = serde_json::json!({"retry_count": 0});
        let error = provider_json_with_retry(&config, &|| false, || {
            Ok(ProviderHttpResponse {
                body: "not json".into(),
                status: Some(200),
                retry_after_ms: None,
                time_to_first_byte_ms: Some(33.5),
                request_total_ms: Some(72.25),
            })
        })
        .unwrap_err();

        assert!(
            error.contains("provider_time_to_first_byte_ms 33.5"),
            "{error}"
        );
        assert!(error.contains("provider_request_total_ms 72.25"), "{error}");

        let metrics = provider_error_transport_metrics(&error);
        assert_eq!(metrics.get("http_status"), Some(&serde_json::json!(200)));
        assert_eq!(
            metrics.get("provider_time_to_first_byte_ms"),
            Some(&serde_json::json!(33.5))
        );
        assert_eq!(
            metrics.get("provider_request_total_ms"),
            Some(&serde_json::json!(72.25))
        );
    }

    #[test]
    fn parses_curl_http_status_marker() {
        let parsed = parse_curl_http_response(
            "hello\n__BENCHFORGE_HTTP_STATUS__:201\n__BENCHFORGE_TIME_STARTTRANSFER__:0.123456\n__BENCHFORGE_TIME_TOTAL__:0.200000",
        );
        assert_eq!(parsed.body, "hello");
        assert_eq!(parsed.status, Some(201));
        assert_eq!(parsed.retry_after_ms, None);
        assert_eq!(parsed.time_to_first_byte_ms, Some(123.456));
        assert_eq!(parsed.request_total_ms, Some(200.0));

        let no_marker = parse_curl_http_response("plain body");
        assert_eq!(no_marker.body, "plain body");
        assert_eq!(no_marker.status, None);
        assert_eq!(no_marker.retry_after_ms, None);
        assert_eq!(no_marker.time_to_first_byte_ms, None);
        assert_eq!(no_marker.request_total_ms, None);
    }

    #[test]
    fn parses_retry_after_headers_and_caps_delay() {
        let now = DateTime::parse_from_rfc2822("Wed, 21 Oct 2015 07:28:00 GMT")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            parse_retry_after_ms_at("HTTP/1.1 429\r\nRetry-After: 2\r\n", now),
            Some(2_000)
        );
        assert_eq!(
            parse_retry_after_ms_at(
                "HTTP/1.1 429\r\nRetry-After: Wed, 21 Oct 2015 07:28:02 GMT\r\n",
                now
            ),
            Some(2_000)
        );
        assert_eq!(
            parse_retry_after_ms_at("Retry-After: 120\r\n", now),
            Some(MAX_PROVIDER_RETRY_AFTER_MS)
        );
        assert_eq!(
            provider_retry_delay_ms(2, Some(90_000)),
            MAX_PROVIDER_RETRY_AFTER_MS
        );
        assert_eq!(provider_retry_delay_ms(2, None), 500);
    }

    #[test]
    fn sandboxed_scoring_env_uses_workspace_paths_without_secret_vars() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-sandbox-env-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp workspace");
        let env = sandboxed_scoring_env(&dir).expect("build sandbox env");
        assert_eq!(
            env.get("HOME").map(String::as_str),
            Some(dir.join(SANDBOX_HOME_DIR).to_string_lossy().as_ref())
        );
        assert_eq!(
            env.get("TMPDIR").map(String::as_str),
            Some(dir.join(SANDBOX_TMP_DIR).to_string_lossy().as_ref())
        );
        assert_eq!(env.get("PATH"), Some(&adapters::gui_path()));
        for secret_name in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "MISTRAL_API_KEY",
            "HF_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "GITHUB_TOKEN",
        ] {
            assert!(!env.contains_key(secret_name));
        }
        assert!(dir.join(SANDBOX_HOME_DIR).is_dir());
        assert!(dir.join(SANDBOX_TMP_DIR).is_dir());
        assert!(dir.join(SANDBOX_NPM_CACHE_DIR).is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn provider_json_uses_http_status_for_errors() {
        let config = serde_json::json!({"retry_count": 0});
        let error = provider_json_with_retry(&config, &|| false, || {
            Ok(ProviderHttpResponse {
                body: "Too many requests".into(),
                status: Some(429),
                retry_after_ms: None,
                time_to_first_byte_ms: None,
                request_total_ms: None,
            })
        })
        .unwrap_err();
        assert!(error.contains("http_status 429"));
        assert_eq!(normalize_provider_error_code(&error), "rate_limit");

        let error = provider_json_with_retry(&config, &|| false, || {
            Ok(ProviderHttpResponse {
                body: r#"{"error":{"type":"invalid_api_key","message":"bad key"}}"#.into(),
                status: Some(401),
                retry_after_ms: None,
                time_to_first_byte_ms: None,
                request_total_ms: None,
            })
        })
        .unwrap_err();
        assert!(error.contains("http_status 401"));
        assert_eq!(normalize_provider_error_code(&error), "auth");
    }

    #[test]
    fn provider_error_transport_metrics_extracts_http_status_and_retry_after() {
        let metrics = provider_error_transport_metrics(
            "provider_attempts 2 retry_after_ms 2000 provider_retry_delay_ms 250: http_status 429: rate_limit: Too many requests",
        );
        assert_eq!(
            metrics.get("provider_attempts"),
            Some(&serde_json::json!(2))
        );
        assert_eq!(metrics.get("http_status"), Some(&serde_json::json!(429)));
        assert_eq!(
            metrics.get("provider_retry_after_ms"),
            Some(&serde_json::json!(2000))
        );
        assert_eq!(
            metrics.get("provider_retry_delay_ms"),
            Some(&serde_json::json!(250))
        );
        assert!(provider_error_transport_metrics("network: disconnected").is_empty());
    }

    #[test]
    fn provider_stream_error_preserves_transport_timing_metrics() {
        let response = ProviderStreamResponse {
            content: "partial".into(),
            raw: "event: response.failed\n".into(),
            metrics: serde_json::Map::new(),
            attempts: 0,
            http_status: Some(200),
            retry_after_ms: None,
            retry_delay_ms: None,
            time_to_first_byte_ms: Some(42.5),
            time_to_first_token_ms: Some(88.25),
            request_total_ms: Some(125.75),
        };
        let error = format_provider_stream_error_with_transport(
            "provider_error: stream_failed: terminal failure".into(),
            &response,
        );

        assert!(error.contains("http_status 200"), "{error}");
        assert!(
            error.contains("provider_time_to_first_byte_ms 42.5"),
            "{error}"
        );
        assert!(
            error.contains("provider_time_to_first_token_ms 88.25"),
            "{error}"
        );
        assert!(
            error.contains("provider_request_total_ms 125.75"),
            "{error}"
        );

        let metrics = provider_error_transport_metrics(&error);
        assert_eq!(metrics.get("http_status"), Some(&serde_json::json!(200)));
        assert_eq!(
            metrics.get("provider_time_to_first_byte_ms"),
            Some(&serde_json::json!(42.5))
        );
        assert_eq!(
            metrics.get("provider_time_to_first_token_ms"),
            Some(&serde_json::json!(88.25))
        );
        assert_eq!(
            metrics.get("provider_request_total_ms"),
            Some(&serde_json::json!(125.75))
        );
    }

    #[test]
    fn failed_prompt_run_persists_provider_error_transport_metrics() {
        let server = CloudContractServer::start().expect("contract server should start");
        let _api_key = ScopedEnvVar::set(CLOUD_CONTRACT_API_KEY_ENV, "benchforge-contract-key");
        let conn = store::open_memory().expect("db should open");
        let pack = load_pack("cloud-contract").expect("cloud contract pack should load");
        let mut tasks = load_tasks(&pack).expect("cloud contract task should load");
        let task = tasks.pop().expect("cloud contract task should exist");
        let target = test_model_target(
            "contract-rate-limit",
            "openai-compatible",
            serde_json::json!({
                "model": "contract-rate-limit",
                "base_url": format!("{}/v1", server.base_url),
                "api_key_env": CLOUD_CONTRACT_API_KEY_ENV,
                "retry_count": 0,
                "timeout_seconds": 10,
                "max_tokens": 16
            }),
        );

        let result = run_prompt_task(&conn, &target, &pack, &task, 0, 1, None, &|| false)
            .expect("failed provider call should still persist a run result");
        assert_eq!(result.status, "error");
        assert!(result
            .error
            .as_deref()
            .unwrap_or("")
            .contains("http_status 429"));

        let records = store::list_results(&conn).expect("results should list");
        let record = records
            .iter()
            .find(|record| record.target_id == "contract-rate-limit")
            .expect("failed contract result should persist");
        assert_eq!(record.error_code.as_deref(), Some("rate_limit"));
        assert_eq!(record.http_status, Some(429.0));
        assert_eq!(record.provider_retry_after_ms, Some(2_000.0));
        assert_eq!(record.provider_attempts, Some(1.0));
    }

    #[test]
    fn extracts_provider_error_from_json() {
        let json = serde_json::json!({
            "error": {
                "type": "rate_limit_error",
                "message": "Too many requests"
            }
        });
        assert_eq!(
            provider_error_from_json(&json).as_deref(),
            Some("provider_error: rate_limit_error: Too many requests")
        );
    }

    #[test]
    fn normalizes_provider_error_codes() {
        assert_eq!(
            normalize_provider_error_code("401 unauthorized invalid api key"),
            "auth"
        );
        assert_eq!(
            normalize_provider_error_code("429 rate_limit_error too many requests"),
            "rate_limit"
        );
        assert_eq!(
            normalize_provider_error_code("model_not_found: model does not exist"),
            "model_not_found"
        );
        assert_eq!(
            normalize_provider_error_code("maximum context length exceeded"),
            "context_overflow"
        );
        assert_eq!(
            normalize_provider_error_code("content_filter blocked by safety policy"),
            "content_filter"
        );
        assert_eq!(
            normalize_provider_error_code("malformed_response: invalid provider JSON"),
            "malformed_response"
        );
        assert_eq!(
            normalize_provider_error_code("500 internal error"),
            "server_error"
        );
        assert_eq!(
            normalize_provider_error_code("failed to connect to localhost"),
            "network"
        );
        assert!(should_retry_provider_error("429 rate_limit_error"));
        assert!(should_retry_provider_error("502 bad gateway"));
        assert!(!should_retry_provider_error(
            "401 unauthorized invalid api key"
        ));
    }

    #[test]
    fn command_timeout_is_enforced() {
        let capture = run_command_capture(
            command_at(
                &paths::repo_root(),
                "python3",
                &["-c", "import time; time.sleep(5)"],
            ),
            Duration::from_millis(200),
        )
        .expect("command should return timeout capture");
        assert!(capture.timed_out);
    }

    #[test]
    fn command_timeout_runs_forced_cleanup() {
        let cleanup_called = Arc::new(AtomicBool::new(false));
        let cleanup_flag = cleanup_called.clone();
        let capture = run_command_capture_checked_with_cleanup(
            command_at(
                &paths::repo_root(),
                "python3",
                &["-c", "import time; time.sleep(5)"],
            ),
            Duration::from_millis(200),
            &|| false,
            move || {
                cleanup_flag.store(true, Ordering::SeqCst);
            },
        )
        .expect("command should return timeout capture");

        assert!(capture.timed_out);
        assert!(cleanup_called.load(Ordering::SeqCst));
    }

    #[test]
    fn command_cancellation_kills_process() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_flag = cancelled.clone();
        let cleanup_called = Arc::new(AtomicBool::new(false));
        let cleanup_flag = cleanup_called.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            cancel_flag.store(true, Ordering::SeqCst);
        });
        let result = run_command_capture_checked_with_cleanup(
            command_at(
                &paths::repo_root(),
                "python3",
                &["-c", "import time; time.sleep(5)"],
            ),
            Duration::from_secs(10),
            &|| cancelled.load(Ordering::SeqCst),
            move || {
                cleanup_flag.store(true, Ordering::SeqCst);
            },
        );
        assert_eq!(result.unwrap_err(), "cancelled");
        assert!(cleanup_called.load(Ordering::SeqCst));
    }

    #[test]
    fn applies_json_file_edits() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-model-edit-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        apply_model_output(
            &dir,
            r#"{"edits":[{"path":"src/main.py","content":"print('ok')\n"}]}"#,
            &|| false,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("src/main.py")).unwrap(),
            "print('ok')\n"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
