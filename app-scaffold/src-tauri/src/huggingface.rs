use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::{adapters, paths, safety, store};

const KEYCHAIN_SERVICE: &str = "benchforge/huggingface";
const DISK_SPACE_WARNING_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
struct DownloadPlan {
    summary: String,
    planned_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
struct PreparedDownload {
    selected_file: String,
    revision: Option<String>,
    model_dir: PathBuf,
    hf_home_dir: PathBuf,
    expected_file: PathBuf,
    expected_sha256: Option<String>,
    plan: DownloadPlan,
    disk_space_log: String,
    existing_bytes: Option<u64>,
    partial_bytes: u64,
    already_downloaded: bool,
    existing_integrity_log: Option<String>,
    remove_existing_before_download: bool,
}

#[derive(Debug, Clone, Default)]
struct ExistingDownloadValidation {
    reusable: bool,
    log: Option<String>,
    remove_before_download: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HuggingFaceStatusDto {
    #[serde(rename = "tokenAvailable")]
    pub token_available: bool,
    #[serde(rename = "pythonAvailable", default)]
    pub python_available: bool,
    #[serde(rename = "pythonSupported", default)]
    pub python_supported: bool,
    #[serde(rename = "pythonVersion", default)]
    pub python_version: Option<String>,
    #[serde(rename = "hfCliAvailable")]
    pub hf_cli_available: bool,
    #[serde(rename = "llamaServerAvailable")]
    pub llama_server_available: bool,
    #[serde(rename = "serverRunning")]
    pub server_running: bool,
    #[serde(rename = "serverModelId")]
    pub server_model_id: Option<String>,
    #[serde(rename = "cacheDir", default = "default_cache_dir")]
    pub cache_dir: String,
    #[serde(rename = "cacheSizeBytes", default)]
    pub cache_size_bytes: u64,
    #[serde(rename = "cacheFreeBytes", default)]
    pub cache_free_bytes: Option<u64>,
    pub detail: String,
    pub models: Vec<DownloadedModelDto>,
}

fn default_cache_dir() -> String {
    models_root().to_string_lossy().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedModelDto {
    #[serde(rename = "repoId")]
    pub repo_id: String,
    #[serde(default)]
    pub revision: Option<String>,
    pub path: String,
    pub files: Vec<String>,
    #[serde(rename = "ggufFiles")]
    pub gguf_files: Vec<String>,
    #[serde(rename = "ggufFileDetails")]
    pub gguf_file_details: Vec<GgufFileDto>,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
    #[serde(rename = "selectedFile")]
    pub selected_file: Option<String>,
    #[serde(rename = "downloadLog")]
    pub download_log: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadModelPlanDto {
    pub repo_id: String,
    pub selected_file: String,
    pub revision: Option<String>,
    pub local_dir: String,
    pub planned_bytes: Option<u64>,
    pub existing_bytes: Option<u64>,
    pub partial_bytes: u64,
    pub already_downloaded: bool,
    pub summary: String,
    pub disk_check: String,
    pub retry_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufFileDto {
    pub file: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub quantization: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModelMetadata {
    #[serde(default)]
    repo_id: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    selected_file: Option<String>,
    #[serde(default)]
    files: BTreeMap<String, StoredGgufFileMetadata>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredGgufFileMetadata {
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    quantization: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HuggingFaceModelDto {
    #[serde(rename = "repoId")]
    pub repo_id: String,
    pub author: Option<String>,
    pub url: String,
    pub downloads: u64,
    pub likes: u64,
    #[serde(rename = "trendingScore")]
    pub trending_score: Option<f64>,
    #[serde(rename = "pipelineTag")]
    pub pipeline_tag: Option<String>,
    #[serde(rename = "libraryName")]
    pub library_name: Option<String>,
    pub gated: bool,
    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,
    pub tags: Vec<String>,
    #[serde(rename = "ggufFiles")]
    pub gguf_files: Vec<String>,
    #[serde(rename = "recommendedFile")]
    pub recommended_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceModelFilesDto {
    pub repo_id: String,
    pub url: String,
    pub gguf_files: Vec<String>,
    pub gguf_file_details: Vec<GgufFileDto>,
    pub recommended_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTokenRequest {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadModelRequest {
    pub repo_id: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub download_id: Option<String>,
    #[serde(default)]
    pub start_after_download: bool,
    #[serde(default)]
    pub run_connectivity_after_start: bool,
    #[serde(default)]
    pub auto_benchmark_pack_id: Option<String>,
    #[serde(default)]
    pub auto_compare_after_start: bool,
    #[serde(default)]
    pub auto_benchmark_target_ids: Vec<String>,
    #[serde(default)]
    pub start_port: Option<u16>,
    #[serde(default)]
    pub start_context: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressDto {
    pub download_id: Option<String>,
    pub repo_id: String,
    pub selected_file: String,
    pub status: String,
    pub message: String,
    pub local_dir: String,
    pub transferred_bytes: u64,
    pub planned_bytes: Option<u64>,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceDownloadJobDto {
    pub id: String,
    pub repo_id: String,
    pub selected_file: Option<String>,
    pub status: String,
    pub message: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub planned_bytes: Option<u64>,
    pub transferred_bytes: u64,
    pub percent: Option<f64>,
    pub local_dir: Option<String>,
    pub error: Option<String>,
    pub model: Option<DownloadedModelDto>,
    pub start_after_download: bool,
    pub run_connectivity_after_start: bool,
    pub auto_benchmark_pack_id: Option<String>,
    pub auto_compare_after_start: bool,
    pub auto_benchmark_target_ids: Vec<String>,
    pub start_port: Option<u16>,
    pub start_context: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    pub repo_id: String,
    #[serde(default)]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartModelRequest {
    pub repo_id: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_context")]
    pub context: u32,
    #[serde(default)]
    pub register_target_after_start: bool,
    #[serde(default)]
    pub run_connectivity_after_start: bool,
    #[serde(default)]
    pub auto_benchmark_pack_id: Option<String>,
    #[serde(default)]
    pub auto_compare_after_start: bool,
    #[serde(default)]
    pub auto_benchmark_target_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HuggingFaceServerJobDto {
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
    pub server_status: Option<HuggingFaceStatusDto>,
    pub register_target_after_start: bool,
    pub run_connectivity_after_start: bool,
    pub auto_benchmark_pack_id: Option<String>,
    pub auto_compare_after_start: bool,
    pub auto_benchmark_target_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchModelsRequest {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default = "default_model_sort")]
    pub sort: String,
    #[serde(default = "default_model_limit")]
    pub limit: u8,
    #[serde(default = "default_true")]
    pub gguf_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallToolsRequest {
    #[serde(default)]
    pub install_python: bool,
    #[serde(default)]
    pub install_hf: bool,
    #[serde(default)]
    pub install_llama: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallToolsResultDto {
    pub status: String,
    pub log: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPreflightDto {
    pub status: String,
    pub summary: String,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub repo_id: String,
    pub selected_file: String,
    pub model_size_bytes: u64,
    pub estimated_memory_bytes: u64,
    pub system_memory_bytes: Option<u64>,
    pub context: u32,
}

fn default_port() -> u16 {
    8080
}

fn default_context() -> u32 {
    2048
}

fn default_model_sort() -> String {
    "trendingScore".into()
}

fn default_model_limit() -> u8 {
    20
}

fn default_true() -> bool {
    true
}

pub fn status(state: &store::AppState) -> Result<HuggingFaceStatusDto, String> {
    let mut exited_server_detail = None;
    let running_server = {
        let mut slot = state.hf_server.lock().map_err(|err| err.to_string())?;
        let running_server = if let Some(running) = slot.as_mut() {
            match running.child.try_wait().map_err(|err| err.to_string())? {
                Some(exit_status) => {
                    exited_server_detail = Some(format!(
                        "previous llama-server exited with status {}",
                        exit_status
                    ));
                    None
                }
                None => Some((running.port, running.server_model_id.clone())),
            }
        } else {
            None
        };
        if exited_server_detail.is_some() {
            let _ = slot.take();
        }
        running_server
    };
    let server_port = running_server.as_ref().map(|(port, _)| *port);
    let server_model_id = running_server
        .and_then(|(port, model_id)| model_id.or_else(|| served_model_id(port).ok().flatten()));
    let server_running = server_port.is_some();
    let token_available = read_token().is_some();
    let python_version = python3_version();
    let python_available = python_version.is_some();
    let python_supported = python_version
        .as_deref()
        .map(|version| version_is_at_least(version, 3, 10))
        .unwrap_or(false);
    let hf_cli_available = adapters::command_exists("hf");
    let llama_server_available = adapters::command_exists("llama-server");
    let mut detail = Vec::new();
    if let Some(port) = server_port {
        let model_suffix = server_model_id
            .as_deref()
            .map(|model| format!(" serving {}", model))
            .unwrap_or_default();
        detail.push(format!(
            "llama-server running on 127.0.0.1:{}{}",
            port, model_suffix
        ));
    }
    if let Some(detail_message) = exited_server_detail {
        detail.push(detail_message);
    }
    if !hf_cli_available {
        detail.push(
            "Install Hugging Face CLI for best cache/resume support: curl -LsSf https://hf.co/cli/install.sh | bash -s; public/token downloads can fall back to curl".into(),
        );
        if !python_supported {
            detail.push(
                python_version
                    .as_deref()
                    .map(|version| {
                        format!(
                            "python3 {} is too old for the Hugging Face CLI installer; Python 3.10+ is required",
                            version
                        )
                    })
                    .unwrap_or_else(|| {
                        "python3 was not found; Python 3.10+ is required for the Hugging Face CLI installer"
                            .into()
                    }),
            );
        }
    }
    if !llama_server_available {
        detail.push("Install llama.cpp: brew install llama.cpp".into());
    }
    if !token_available {
        detail.push(
            "HF token not stored; public downloads may still work, gated models need a token"
                .into(),
        );
    }
    let cache_dir = models_root();
    let models = downloaded_models()?;
    let cache_size_bytes = models.iter().map(|model| model.size_bytes).sum();
    let cache_free_bytes = available_disk_bytes(&cache_dir);
    Ok(HuggingFaceStatusDto {
        token_available,
        python_available,
        python_supported,
        python_version,
        hf_cli_available,
        llama_server_available,
        server_running,
        server_model_id,
        cache_dir: cache_dir.to_string_lossy().to_string(),
        cache_size_bytes,
        cache_free_bytes,
        detail: if detail.is_empty() {
            "ready".into()
        } else {
            detail.join("; ")
        },
        models,
    })
}

pub fn save_token(request: SaveTokenRequest) -> Result<(), String> {
    let token = request.token.trim();
    if token.is_empty() {
        return Err("token is empty".into());
    }
    if cfg!(target_os = "macos") {
        let output = Command::new("security")
            .args([
                "add-generic-password",
                "-a",
                "HF_TOKEN",
                "-s",
                KEYCHAIN_SERVICE,
                "-w",
                token,
                "-U",
            ])
            .output()
            .map_err(|err| err.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    } else {
        Err("persistent HF token storage currently uses macOS Keychain; set HF_TOKEN in the environment on this platform".into())
    }
}

pub fn install_tools(request: InstallToolsRequest) -> Result<InstallToolsResultDto, String> {
    if !cfg!(target_os = "macos") {
        return Err("automatic Hugging Face setup is currently only available on macOS".into());
    }

    let mut log = Vec::new();
    let mut attempted_install = false;

    if request.install_python || request.install_hf {
        attempted_install = true;
        ensure_supported_python(&mut log)?;
    }

    if request.install_hf {
        if adapters::command_exists("hf") {
            log.push("hf CLI already installed".to_string());
        } else {
            attempted_install = true;
            let mut cmd = adapters::command_with_gui_path("bash");
            cmd.args(["-c", "curl -LsSf https://hf.co/cli/install.sh | bash -s"]);
            run_install_command("Hugging Face CLI", &mut cmd, &mut log)?;
        }
    }

    if request.install_llama {
        if adapters::command_exists("llama-server") {
            log.push("llama-server already installed".to_string());
        } else {
            if !adapters::command_exists("brew") {
                log.push("Homebrew is required to install llama.cpp automatically".to_string());
                return Err(log.join("\n"));
            }
            attempted_install = true;
            let mut cmd = adapters::command_with_gui_path("brew");
            cmd.args(["install", "llama.cpp"]);
            run_install_command("llama.cpp", &mut cmd, &mut log)?;
        }
    }

    if !request.install_python && !request.install_hf && !request.install_llama {
        log.push("No missing Hugging Face tools were selected for installation".to_string());
    }

    let status = if adapters::command_exists("hf") && adapters::command_exists("llama-server") {
        "ready"
    } else if attempted_install {
        "partial"
    } else {
        "unchanged"
    };

    Ok(InstallToolsResultDto {
        status: status.into(),
        log: log.join("\n"),
    })
}

pub fn search_models(request: SearchModelsRequest) -> Result<Vec<HuggingFaceModelDto>, String> {
    let limit = request.limit.clamp(1, 50);
    let sort = normalize_model_sort(&request.sort);
    let mut url = format!(
        "https://huggingface.co/api/models?limit={}&sort={}&direction=-1&full=true",
        limit, sort
    );
    if request.gguf_only {
        url.push_str("&filter=gguf");
    }
    if let Some(query) = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        url.push_str("&search=");
        url.push_str(&url_encode(query));
    }

    let raw = get_hf_api_url(&url)?;
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|err| format!("invalid Hugging Face response: {}", err))?;
    if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
        return Err(error.to_string());
    }
    let items = json
        .as_array()
        .ok_or_else(|| "unexpected Hugging Face models response".to_string())?;
    let mut models = Vec::new();
    for item in items {
        if let Some(model) = model_from_value(item) {
            models.push(model);
        }
    }
    Ok(models)
}

pub fn inspect_model(request: ModelRequest) -> Result<HuggingFaceModelFilesDto, String> {
    validate_repo_id(&request.repo_id)?;
    let gguf_file_details =
        resolve_repo_gguf_file_details(&request.repo_id, request.revision.as_deref())?;
    let gguf_files = gguf_file_details
        .iter()
        .map(|detail| detail.file.clone())
        .collect::<Vec<_>>();
    Ok(HuggingFaceModelFilesDto {
        repo_id: request.repo_id.clone(),
        url: format!("https://huggingface.co/{}", request.repo_id),
        recommended_file: recommend_gguf_file(&gguf_files),
        gguf_files,
        gguf_file_details,
    })
}

pub fn plan_download(request: DownloadModelRequest) -> Result<DownloadModelPlanDto, String> {
    let prepared = prepare_download(&request)?;
    Ok(prepared_download_dto(&request.repo_id, &prepared))
}

pub fn download_model(request: DownloadModelRequest) -> Result<DownloadedModelDto, String> {
    download_model_with_progress(request, |_| {})
}

pub fn download_model_with_progress(
    request: DownloadModelRequest,
    mut progress: impl FnMut(DownloadProgressDto),
) -> Result<DownloadedModelDto, String> {
    download_model_with_progress_and_cancel(request, &mut progress, || false)
}

fn download_model_with_progress_and_cancel(
    request: DownloadModelRequest,
    progress: &mut impl FnMut(DownloadProgressDto),
    mut cancellation_requested: impl FnMut() -> bool,
) -> Result<DownloadedModelDto, String> {
    if cancellation_requested() {
        return Err("cancelled".into());
    }
    let prepared = prepare_download(&request)?;
    emit_download_progress(
        &request,
        &prepared,
        progress,
        "planned",
        current_download_bytes(&prepared),
        &prepared.plan.summary,
    );
    if prepared.already_downloaded {
        emit_download_progress(
            &request,
            &prepared,
            progress,
            "completed",
            prepared.existing_bytes.unwrap_or(0),
            "Existing file matches the planned Hugging Face download; skipped network transfer.",
        );
        return finalize_downloaded_model(
            &request.repo_id,
            &prepared,
            "Existing file matches the planned Hugging Face download; skipped network transfer."
                .into(),
        );
    }
    if cancellation_requested() {
        emit_download_progress(
            &request,
            &prepared,
            progress,
            "cancelled",
            current_download_bytes(&prepared),
            "Download cancelled before network transfer.",
        );
        return Err("cancelled".into());
    }
    if prepared.remove_existing_before_download && prepared.expected_file.exists() {
        fs::remove_file(&prepared.expected_file).map_err(|err| {
            format!(
                "existing file failed checksum validation, but BenchForge could not remove it before re-download: {}",
                err
            )
        })?;
        emit_download_progress(
            &request,
            &prepared,
            progress,
            "running",
            current_download_bytes(&prepared),
            "Existing GGUF failed checksum validation; re-downloading a clean copy.",
        );
    }
    emit_download_progress(
        &request,
        &prepared,
        progress,
        "running",
        current_download_bytes(&prepared),
        "Hugging Face download started.",
    );

    let transfer_log = if adapters::command_exists("hf") {
        match run_hf_cli_download(&request, &prepared, progress, &mut cancellation_requested) {
            Ok(log) => log,
            Err(err) if err == "cancelled" => return Err(err),
            Err(cli_err) => {
                if !adapters::command_exists("curl") {
                    emit_download_progress(
                        &request,
                        &prepared,
                        progress,
                        "error",
                        current_download_bytes(&prepared),
                        &cli_err,
                    );
                    return Err(cli_err);
                }
                emit_download_progress(
                    &request,
                    &prepared,
                    progress,
                    "running",
                    current_download_bytes(&prepared),
                    "hf CLI download failed; trying direct Hugging Face file download with curl.",
                );
                let curl_log = match run_curl_download(
                    &request,
                    &prepared,
                    progress,
                    &mut cancellation_requested,
                ) {
                    Ok(log) => log,
                    Err(err) if err == "cancelled" => return Err(err),
                    Err(err) => {
                        emit_download_progress(
                            &request,
                            &prepared,
                            progress,
                            "error",
                            current_download_bytes(&prepared),
                            &err,
                        );
                        return Err(err);
                    }
                };
                join_non_empty(&[
                    format!("hf CLI download failed; used curl fallback:\n{}", cli_err),
                    curl_log,
                ])
            }
        }
    } else {
        match run_curl_download(&request, &prepared, progress, &mut cancellation_requested) {
            Ok(log) => log,
            Err(err) if err == "cancelled" => return Err(err),
            Err(err) => {
                emit_download_progress(
                    &request,
                    &prepared,
                    progress,
                    "error",
                    current_download_bytes(&prepared),
                    &err,
                );
                return Err(err);
            }
        }
    };

    if !prepared.expected_file.exists() {
        let message = format!(
            "download completed, but expected file was not found: {}",
            prepared.selected_file
        );
        emit_download_progress(
            &request,
            &prepared,
            progress,
            "error",
            current_download_bytes(&prepared),
            &message,
        );
        return Err(format!(
            "download completed, but expected file was not found: {}\n{}",
            prepared.selected_file,
            join_non_empty(&[transfer_log, download_retry_hint(&prepared)])
        ));
    }
    let downloaded =
        finalize_downloaded_model(&request.repo_id, &prepared, transfer_log).map_err(|err| {
            emit_download_progress(
                &request,
                &prepared,
                progress,
                "error",
                current_download_bytes(&prepared),
                &err,
            );
            err
        })?;
    emit_download_progress(
        &request,
        &prepared,
        progress,
        "completed",
        current_download_bytes(&prepared),
        "Download completed.",
    );
    Ok(downloaded)
}

fn run_hf_cli_download(
    request: &DownloadModelRequest,
    prepared: &PreparedDownload,
    progress: &mut impl FnMut(DownloadProgressDto),
    cancellation_requested: &mut impl FnMut() -> bool,
) -> Result<String, String> {
    let mut args = vec!["download".to_string(), request.repo_id.clone()];
    args.push(prepared.selected_file.clone());
    args.push("--local-dir".to_string());
    args.push(prepared.model_dir.to_string_lossy().to_string());
    if let Some(revision) = &request.revision {
        if !revision.trim().is_empty() {
            args.push("--revision".into());
            args.push(revision.clone());
        }
    }
    args.push("--json".into());
    let mut cmd = adapters::command_with_gui_path("hf");
    cmd.args(args)
        .env("HF_HOME", &prepared.hf_home_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(token) = read_token() {
        cmd.env("HF_TOKEN", token);
    }
    run_download_command(
        cmd,
        request,
        prepared,
        progress,
        cancellation_requested,
        "Downloading model file...",
    )
}

fn run_curl_download(
    request: &DownloadModelRequest,
    prepared: &PreparedDownload,
    progress: &mut impl FnMut(DownloadProgressDto),
    cancellation_requested: &mut impl FnMut() -> bool,
) -> Result<String, String> {
    if let Some(parent) = prepared.expected_file.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let url = hf_resolve_url(
        &request.repo_id,
        request.revision.as_deref(),
        &prepared.selected_file,
    );
    let mut cmd = adapters::command_with_gui_path("curl");
    cmd.args(["-fL", "--retry", "2", "--connect-timeout", "30"]);
    cmd.args(["-A", "BenchForge/0.1", "-C", "-"]);
    if let Some(token) = read_token() {
        cmd.arg("-H")
            .arg(format!("Authorization: Bearer {}", token));
    }
    cmd.arg("-o")
        .arg(&prepared.expected_file)
        .arg(&url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_download_command(
        cmd,
        request,
        prepared,
        progress,
        cancellation_requested,
        "Downloading model file with curl...",
    )
}

fn run_download_command(
    mut cmd: Command,
    request: &DownloadModelRequest,
    prepared: &PreparedDownload,
    progress: &mut impl FnMut(DownloadProgressDto),
    cancellation_requested: &mut impl FnMut() -> bool,
    running_message: &str,
) -> Result<String, String> {
    let mut child = cmd.spawn().map_err(|err| err.to_string())?;
    let stdout_reader = child.stdout.take().map(read_pipe_in_thread);
    let stderr_reader = child.stderr.take().map(read_pipe_in_thread);
    let mut last_emit = Instant::now();
    let status = loop {
        if cancellation_requested() {
            store::terminate_child_process(&mut child);
            emit_download_progress(
                &request,
                &prepared,
                progress,
                "cancelled",
                current_download_bytes(&prepared),
                "Download cancelled.",
            );
            return Err("cancelled".into());
        }
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            break status;
        }
        if last_emit.elapsed() >= Duration::from_millis(500) {
            emit_download_progress(
                request,
                prepared,
                progress,
                "running",
                current_download_bytes(prepared),
                running_message,
            );
            last_emit = Instant::now();
        }
        thread::sleep(Duration::from_millis(100));
    };
    let stdout = join_pipe_reader(stdout_reader)?;
    let stderr = join_pipe_reader(stderr_reader)?;
    if !status.success() {
        let log = join_non_empty(&[
            command_log(&stdout, &stderr),
            download_retry_hint(&prepared),
        ]);
        return Err(friendly_download_error(&log));
    }
    Ok(command_log(&stdout, &stderr))
}

pub fn start_download_job(
    conn: &Connection,
    mut request: DownloadModelRequest,
) -> Result<HuggingFaceDownloadJobDto, String> {
    validate_repo_id(&request.repo_id)?;
    let selected_file = normalized_optional_filename(request.filename.as_deref())?;
    request.auto_benchmark_target_ids =
        normalized_benchmark_target_ids(&request.auto_benchmark_target_ids);
    if request.start_after_download {
        validate_local_server_settings(
            request.start_port.unwrap_or_else(default_port),
            request.start_context.unwrap_or_else(default_context),
        )?;
    }
    let job_id = uuid::Uuid::new_v4().to_string();
    let started_at = store::now();
    let record = store::HfDownloadJobRecord {
        id: job_id.clone(),
        repo_id: request.repo_id.clone(),
        selected_file,
        status: "queued".into(),
        message: format!("Queued Hugging Face download for {}", request.repo_id),
        started_at,
        finished_at: None,
        planned_bytes: None,
        transferred_bytes: 0,
        local_dir: None,
        error: None,
        request: serde_json::to_value(&request).unwrap_or_else(|_| serde_json::json!({})),
        model: None,
    };
    store::insert_hf_download_job(conn, &record).map_err(|err| err.to_string())?;

    let worker_job_id = job_id.clone();
    std::thread::spawn(move || {
        let Ok(conn) = store::open_app() else {
            return;
        };
        if download_job_cancellation_requested(&conn, &worker_job_id) {
            finish_cancelled_download_job(&conn, &worker_job_id);
            return;
        }
        let _ = store::update_hf_download_job_progress(
            &conn,
            &worker_job_id,
            "running",
            "Planning Hugging Face download",
            None,
            None,
            0,
            None,
        );
        let progress_job_id = worker_job_id.clone();
        let result = download_model_with_progress_and_cancel(
            request,
            &mut |progress| {
                if !download_job_cancellation_requested(&conn, &progress_job_id) {
                    let _ = store::update_hf_download_job_progress(
                        &conn,
                        &progress_job_id,
                        "running",
                        &progress.message,
                        Some(&progress.selected_file),
                        progress.planned_bytes,
                        progress.transferred_bytes,
                        Some(&progress.local_dir),
                    );
                }
            },
            || download_job_cancellation_requested(&conn, &worker_job_id),
        );
        let finished_at = store::now();
        match result {
            Ok(model) => {
                let model_json = serde_json::to_value(&model).ok();
                let message = format!(
                    "Downloaded {} / {}",
                    model.repo_id,
                    model
                        .selected_file
                        .as_deref()
                        .unwrap_or("selected GGUF model")
                );
                let _ = store::finish_hf_download_job(
                    &conn,
                    &worker_job_id,
                    "completed",
                    &message,
                    &finished_at,
                    None,
                    model_json.as_ref(),
                );
            }
            Err(err) if err == "cancelled" => {
                finish_cancelled_download_job(&conn, &worker_job_id);
            }
            Err(err) => {
                let _ = store::finish_hf_download_job(
                    &conn,
                    &worker_job_id,
                    "failed",
                    &err,
                    &finished_at,
                    Some(&err),
                    None,
                );
            }
        }
    });

    get_download_job(conn, &job_id)?.ok_or_else(|| "download job was not persisted".into())
}

pub fn list_download_jobs(conn: &Connection) -> Result<Vec<HuggingFaceDownloadJobDto>, String> {
    store::list_hf_download_jobs(conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(download_job_from_record)
        .collect()
}

pub fn get_download_job(
    conn: &Connection,
    id: &str,
) -> Result<Option<HuggingFaceDownloadJobDto>, String> {
    store::get_hf_download_job(conn, id)
        .map_err(|err| err.to_string())?
        .map(download_job_from_record)
        .transpose()
}

pub fn cancel_download_job(
    conn: &Connection,
    id: &str,
) -> Result<Option<HuggingFaceDownloadJobDto>, String> {
    let Some(job) = store::get_hf_download_job(conn, id).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if matches!(job.status.as_str(), "queued" | "running") {
        store::request_cancel_hf_download_job(conn, id).map_err(|err| err.to_string())?;
    }
    get_download_job(conn, id)
}

pub fn retry_download_job(
    conn: &Connection,
    id: &str,
) -> Result<Option<HuggingFaceDownloadJobDto>, String> {
    let Some(job) = store::get_hf_download_job(conn, id).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if hf_job_is_active(&job.status) {
        return Err(
            "active_hf_download_retry_blocked: wait for the download job to finish or cancel it before retrying"
                .into(),
        );
    }
    if !matches!(job.status.as_str(), "failed" | "cancelled") {
        return Err("only failed or cancelled download jobs can be retried".into());
    }
    let request = download_request_from_job(&job);
    start_download_job(conn, request).map(Some)
}

pub fn clear_finished_download_jobs(conn: &Connection) -> Result<usize, String> {
    store::clear_terminal_hf_download_jobs(conn).map_err(|err| err.to_string())
}

fn download_job_from_record(
    record: store::HfDownloadJobRecord,
) -> Result<HuggingFaceDownloadJobDto, String> {
    let start_after_download = request_bool(&record.request, "startAfterDownload");
    let run_connectivity_after_start = request_bool(&record.request, "runConnectivityAfterStart");
    let auto_benchmark_pack_id =
        auto_benchmark_pack_from_request(&record.request, run_connectivity_after_start);
    let auto_compare_after_start = request_bool(&record.request, "autoCompareAfterStart");
    let auto_benchmark_target_ids = request_string_array(&record.request, "autoBenchmarkTargetIds");
    let start_port = request_u16(&record.request, "startPort");
    let start_context = request_u32(&record.request, "startContext");
    let model = record
        .model
        .map(serde_json::from_value::<DownloadedModelDto>)
        .transpose()
        .map_err(|err| err.to_string())?;
    let percent = if record.status == "completed" {
        Some(100.0)
    } else {
        record.planned_bytes.and_then(|planned| {
            (planned > 0).then(|| {
                ((record.transferred_bytes as f64 / planned as f64) * 100.0).clamp(0.0, 100.0)
            })
        })
    };
    Ok(HuggingFaceDownloadJobDto {
        id: record.id,
        repo_id: record.repo_id,
        selected_file: record.selected_file,
        status: record.status,
        message: record.message,
        started_at: record.started_at,
        finished_at: record.finished_at,
        planned_bytes: record.planned_bytes,
        transferred_bytes: record.transferred_bytes,
        percent,
        local_dir: record.local_dir,
        error: record.error,
        model,
        start_after_download,
        run_connectivity_after_start,
        auto_benchmark_pack_id,
        auto_compare_after_start,
        auto_benchmark_target_ids,
        start_port,
        start_context,
    })
}

fn download_job_cancellation_requested(conn: &Connection, id: &str) -> bool {
    store::hf_download_job_cancellation_requested(conn, id).unwrap_or(false)
}

fn finish_cancelled_download_job(conn: &Connection, id: &str) {
    let finished_at = store::now();
    let _ = store::finish_hf_download_job(
        conn,
        id,
        "cancelled",
        "Cancelled by user",
        &finished_at,
        Some("cancelled"),
        None,
    );
}

fn download_request_from_job(record: &store::HfDownloadJobRecord) -> DownloadModelRequest {
    let mut request = serde_json::from_value::<DownloadModelRequest>(record.request.clone())
        .unwrap_or_else(|_| DownloadModelRequest {
            repo_id: record.repo_id.clone(),
            filename: record.selected_file.clone(),
            revision: request_string(&record.request, "revision"),
            download_id: None,
            start_after_download: request_bool(&record.request, "startAfterDownload"),
            run_connectivity_after_start: request_bool(
                &record.request,
                "runConnectivityAfterStart",
            ),
            auto_benchmark_pack_id: auto_benchmark_pack_from_request(
                &record.request,
                request_bool(&record.request, "runConnectivityAfterStart"),
            ),
            auto_compare_after_start: request_bool(&record.request, "autoCompareAfterStart"),
            auto_benchmark_target_ids: request_string_array(
                &record.request,
                "autoBenchmarkTargetIds",
            ),
            start_port: request_u16(&record.request, "startPort"),
            start_context: request_u32(&record.request, "startContext"),
        });
    if request.repo_id.trim().is_empty() {
        request.repo_id = record.repo_id.clone();
    }
    if request
        .filename
        .as_deref()
        .map_or(true, |filename| filename.trim().is_empty())
    {
        request.filename = record.selected_file.clone();
    }
    request.download_id = None;
    request
}

pub fn normalize_start_request(
    mut request: StartModelRequest,
) -> Result<StartModelRequest, String> {
    validate_repo_id(&request.repo_id)?;
    request.filename = normalized_optional_filename(request.filename.as_deref())?;
    request.auto_benchmark_target_ids =
        normalized_benchmark_target_ids(&request.auto_benchmark_target_ids);
    validate_local_server_settings(request.port, request.context)?;
    Ok(request)
}

pub fn enqueue_server_job(
    conn: &Connection,
    request: StartModelRequest,
) -> Result<HuggingFaceServerJobDto, String> {
    let request = normalize_start_request(request)?;
    let job_id = uuid::Uuid::new_v4().to_string();
    let started_at = store::now();
    let record = store::HfServerJobRecord {
        id: job_id.clone(),
        repo_id: request.repo_id.clone(),
        selected_file: request.filename.clone(),
        port: request.port,
        context: request.context,
        status: "queued".into(),
        message: format!("Queued llama-server start for {}", request.repo_id),
        started_at,
        finished_at: None,
        error: None,
        request: serde_json::to_value(&request).unwrap_or_else(|_| serde_json::json!({})),
        server_status: None,
    };
    store::insert_hf_server_job(conn, &record).map_err(|err| err.to_string())?;
    get_server_job(conn, &job_id)?.ok_or_else(|| "server start job was not persisted".into())
}

pub fn run_server_job(state: &store::AppState, id: String, request: StartModelRequest) {
    let Ok(conn) = store::open_app() else {
        return;
    };
    let request = match normalize_start_request(request) {
        Ok(request) => request,
        Err(err) => {
            finish_failed_server_job(&conn, &id, &err);
            return;
        }
    };
    if server_job_cancellation_requested(&conn, &id) {
        finish_cancelled_server_job(&conn, &id);
        return;
    }

    let preflight = match preflight_model(request.clone()) {
        Ok(preflight) => preflight,
        Err(err) => {
            finish_failed_server_job(&conn, &id, &err);
            return;
        }
    };
    let start_message = format!(
        "Starting llama-server on 127.0.0.1:{} with {}",
        request.port, preflight.selected_file
    );
    let _ = store::update_hf_server_job_progress(
        &conn,
        &id,
        "running",
        &start_message,
        Some(&preflight.selected_file),
    );
    if server_job_cancellation_requested(&conn, &id) {
        finish_cancelled_server_job(&conn, &id);
        return;
    }

    let cancellation_job_id = id.clone();
    let result = start_server_with_cancel(state, request.clone(), || {
        server_job_cancellation_requested(&conn, &cancellation_job_id)
    });
    let finished_at = store::now();
    match result {
        Ok(server_status) => {
            let server_status_json = serde_json::to_value(&server_status).ok();
            let message = format!(
                "llama-server ready on 127.0.0.1:{} for {}",
                request.port, preflight.selected_file
            );
            let _ = store::finish_hf_server_job(
                &conn,
                &id,
                "completed",
                &message,
                &finished_at,
                None,
                server_status_json.as_ref(),
            );
        }
        Err(err) if err == "cancelled" => finish_cancelled_server_job(&conn, &id),
        Err(err) => {
            let _ = store::finish_hf_server_job(
                &conn,
                &id,
                "failed",
                &err,
                &finished_at,
                Some(&err),
                None,
            );
        }
    }
}

pub fn list_server_jobs(conn: &Connection) -> Result<Vec<HuggingFaceServerJobDto>, String> {
    store::list_hf_server_jobs(conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(server_job_from_record)
        .collect()
}

pub fn get_server_job(
    conn: &Connection,
    id: &str,
) -> Result<Option<HuggingFaceServerJobDto>, String> {
    store::get_hf_server_job(conn, id)
        .map_err(|err| err.to_string())?
        .map(server_job_from_record)
        .transpose()
}

pub fn cancel_server_job(
    conn: &Connection,
    id: &str,
) -> Result<Option<HuggingFaceServerJobDto>, String> {
    let Some(job) = store::get_hf_server_job(conn, id).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if matches!(job.status.as_str(), "queued" | "running") {
        store::request_cancel_hf_server_job(conn, id).map_err(|err| err.to_string())?;
    }
    get_server_job(conn, id)
}

pub fn retry_server_job(
    conn: &Connection,
    id: &str,
) -> Result<Option<(HuggingFaceServerJobDto, StartModelRequest)>, String> {
    let Some(job) = store::get_hf_server_job(conn, id).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if hf_job_is_active(&job.status) {
        return Err(
            "active_hf_server_retry_blocked: wait for the server start job to finish or cancel it before retrying"
                .into(),
        );
    }
    if !matches!(job.status.as_str(), "failed" | "cancelled") {
        return Err("only failed or cancelled server start jobs can be retried".into());
    }
    let request = server_request_from_job(&job);
    let retry = enqueue_server_job(conn, request.clone())?;
    Ok(Some((retry, request)))
}

pub fn clear_finished_server_jobs(conn: &Connection) -> Result<usize, String> {
    store::clear_terminal_hf_server_jobs(conn).map_err(|err| err.to_string())
}

fn server_job_from_record(
    record: store::HfServerJobRecord,
) -> Result<HuggingFaceServerJobDto, String> {
    let register_target_after_start = request_bool(&record.request, "registerTargetAfterStart");
    let run_connectivity_after_start = request_bool(&record.request, "runConnectivityAfterStart");
    let auto_benchmark_pack_id =
        auto_benchmark_pack_from_request(&record.request, run_connectivity_after_start);
    let auto_compare_after_start = request_bool(&record.request, "autoCompareAfterStart");
    let auto_benchmark_target_ids = request_string_array(&record.request, "autoBenchmarkTargetIds");
    let server_status = record
        .server_status
        .map(serde_json::from_value::<HuggingFaceStatusDto>)
        .transpose()
        .map_err(|err| err.to_string())?;
    Ok(HuggingFaceServerJobDto {
        id: record.id,
        repo_id: record.repo_id,
        selected_file: record.selected_file,
        port: record.port,
        context: record.context,
        status: record.status,
        message: record.message,
        started_at: record.started_at,
        finished_at: record.finished_at,
        error: record.error,
        server_status,
        register_target_after_start,
        run_connectivity_after_start,
        auto_benchmark_pack_id,
        auto_compare_after_start,
        auto_benchmark_target_ids,
    })
}

fn request_bool(request: &serde_json::Value, field: &str) -> bool {
    request
        .get(field)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn request_u16(request: &serde_json::Value, field: &str) -> Option<u16> {
    request
        .get(field)
        .and_then(|value| value.as_u64())
        .and_then(|value| u16::try_from(value).ok())
}

fn request_u32(request: &serde_json::Value, field: &str) -> Option<u32> {
    request
        .get(field)
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
}

fn request_string(request: &serde_json::Value, field: &str) -> Option<String> {
    request
        .get(field)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn request_string_array(request: &serde_json::Value, field: &str) -> Vec<String> {
    let values = request
        .get(field)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    normalized_benchmark_target_ids(&values)
}

fn normalized_benchmark_target_ids(values: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert((*value).to_string()))
        .map(ToOwned::to_owned)
        .collect()
}

fn auto_benchmark_pack_from_request(
    request: &serde_json::Value,
    run_connectivity_after_start: bool,
) -> Option<String> {
    request_string(request, "autoBenchmarkPackId")
        .or_else(|| run_connectivity_after_start.then(|| "llm-connectivity".to_string()))
}

fn server_request_from_job(record: &store::HfServerJobRecord) -> StartModelRequest {
    let mut request = serde_json::from_value::<StartModelRequest>(record.request.clone())
        .unwrap_or_else(|_| StartModelRequest {
            repo_id: record.repo_id.clone(),
            filename: record.selected_file.clone(),
            port: record.port,
            context: record.context,
            register_target_after_start: request_bool(&record.request, "registerTargetAfterStart"),
            run_connectivity_after_start: request_bool(
                &record.request,
                "runConnectivityAfterStart",
            ),
            auto_benchmark_pack_id: auto_benchmark_pack_from_request(
                &record.request,
                request_bool(&record.request, "runConnectivityAfterStart"),
            ),
            auto_compare_after_start: request_bool(&record.request, "autoCompareAfterStart"),
            auto_benchmark_target_ids: request_string_array(
                &record.request,
                "autoBenchmarkTargetIds",
            ),
        });
    if request.repo_id.trim().is_empty() {
        request.repo_id = record.repo_id.clone();
    }
    if request
        .filename
        .as_deref()
        .map_or(true, |filename| filename.trim().is_empty())
    {
        request.filename = record.selected_file.clone();
    }
    if record.request.get("port").is_none() {
        request.port = record.port;
    }
    if record.request.get("context").is_none() {
        request.context = record.context;
    }
    request
}

fn server_job_cancellation_requested(conn: &Connection, id: &str) -> bool {
    store::hf_server_job_cancellation_requested(conn, id).unwrap_or(false)
}

fn finish_cancelled_server_job(conn: &Connection, id: &str) {
    let finished_at = store::now();
    let _ = store::finish_hf_server_job(
        conn,
        id,
        "cancelled",
        "Cancelled by user",
        &finished_at,
        Some("cancelled"),
        None,
    );
}

fn finish_failed_server_job(conn: &Connection, id: &str, err: &str) {
    let finished_at = store::now();
    let _ = store::finish_hf_server_job(conn, id, "failed", err, &finished_at, Some(err), None);
}

fn emit_download_progress(
    request: &DownloadModelRequest,
    prepared: &PreparedDownload,
    progress: &mut impl FnMut(DownloadProgressDto),
    status: &str,
    transferred_bytes: u64,
    message: &str,
) {
    let percent = prepared.plan.planned_bytes.and_then(|planned| {
        (planned > 0).then(|| ((transferred_bytes as f64 / planned as f64) * 100.0).min(100.0))
    });
    progress(DownloadProgressDto {
        download_id: request.download_id.clone(),
        repo_id: request.repo_id.clone(),
        selected_file: prepared.selected_file.clone(),
        status: status.into(),
        message: truncate_progress_message(message),
        local_dir: prepared.model_dir.to_string_lossy().to_string(),
        transferred_bytes,
        planned_bytes: prepared.plan.planned_bytes,
        percent,
    });
}

fn current_download_bytes(prepared: &PreparedDownload) -> u64 {
    file_size(&prepared.expected_file)
        .unwrap_or(0)
        .max(partial_download_bytes(&prepared.model_dir))
}

fn read_pipe_in_thread<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = reader.read_to_end(&mut bytes);
        bytes
    })
}

fn join_pipe_reader(handle: Option<thread::JoinHandle<Vec<u8>>>) -> Result<Vec<u8>, String> {
    match handle {
        Some(handle) => handle
            .join()
            .map_err(|_| "failed to collect Hugging Face CLI output".to_string()),
        None => Ok(Vec::new()),
    }
}

fn truncate_progress_message(message: &str) -> String {
    const LIMIT: usize = 2400;
    if message.len() <= LIMIT {
        return message.to_string();
    }
    let mut end = LIMIT;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &message[..end])
}

fn finalize_downloaded_model(
    repo_id: &str,
    prepared: &PreparedDownload,
    command_log_text: String,
) -> Result<DownloadedModelDto, String> {
    let mut downloaded_file = gguf_file_detail(&prepared.model_dir, &prepared.selected_file, None)?;
    let actual_sha256 = checksum_file(&prepared.expected_file).map_err(|err| {
        format!(
            "download completed, but BenchForge could not verify {}: {}",
            prepared.selected_file, err
        )
    })?;
    if let Some(expected_sha256) = prepared.expected_sha256.as_deref() {
        if actual_sha256 != expected_sha256 {
            return Err(format!(
                "downloaded GGUF failed Hugging Face SHA-256 validation for {} (expected {}, got {})",
                prepared.selected_file,
                short_hash(expected_sha256),
                short_hash(&actual_sha256)
            ));
        }
    }
    downloaded_file.sha256 = Some(actual_sha256);
    write_metadata(
        repo_id,
        &prepared.model_dir,
        prepared.revision.as_deref(),
        Some(&prepared.selected_file),
        Some(&downloaded_file),
    )?;
    let mut dto = model_dto(repo_id, &prepared.model_dir)?;
    if dto.gguf_files.is_empty() {
        return Err(
            "download completed, but no GGUF files were found in the model directory".into(),
        );
    }
    dto.selected_file = Some(prepared.selected_file.clone());
    dto.download_log = Some(join_non_empty(&[
        prepared.plan.summary.clone(),
        prepared.disk_space_log.clone(),
        download_destination_summary(prepared),
        command_log_text,
    ]));
    Ok(dto)
}

pub fn reveal_model(request: ModelRequest) -> Result<(), String> {
    validate_repo_id(&request.repo_id)?;
    let dir = model_dir(&request.repo_id);
    if !dir.exists() {
        return Err(format!("model {} is not downloaded yet", request.repo_id));
    }
    if cfg!(target_os = "macos") {
        let output = Command::new("open")
            .arg(&dir)
            .output()
            .map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_log(&output.stdout, &output.stderr))
        }
    } else {
        Err(format!(
            "model files are stored at {}",
            dir.to_string_lossy()
        ))
    }
}

pub fn delete_model(
    state: &store::AppState,
    request: ModelRequest,
) -> Result<HuggingFaceStatusDto, String> {
    validate_repo_id(&request.repo_id)?;
    let dir = model_dir(&request.repo_id);
    if !dir.exists() {
        return status(state);
    }
    {
        let conn = state.conn.lock().map_err(|err| err.to_string())?;
        if let Some(blocker) = active_model_lifecycle_job_blocker(&conn, &request.repo_id)? {
            return Err(blocker);
        }
    }
    {
        let slot = state.hf_server.lock().map_err(|err| err.to_string())?;
        if let Some(running) = slot.as_ref() {
            if model_path_is_inside_dir(&running.model_path, &dir) {
                return Err("stop the running local model before deleting its files".into());
            }
        }
    }
    fs::remove_dir_all(&dir).map_err(|err| err.to_string())?;
    status(state)
}

fn active_model_lifecycle_job_blocker(
    conn: &Connection,
    repo_id: &str,
) -> Result<Option<String>, String> {
    if let Some(job) = store::list_hf_download_jobs(conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|job| job.repo_id == repo_id && hf_job_is_active(&job.status))
    {
        return Ok(Some(format!(
            "cancel or wait for download job {} ({}) before deleting {}",
            short_job_id(&job.id),
            job.status,
            repo_id
        )));
    }
    if let Some(job) = store::list_hf_server_jobs(conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|job| job.repo_id == repo_id && hf_job_is_active(&job.status))
    {
        return Ok(Some(format!(
            "cancel or wait for server start job {} ({}) before deleting {}",
            short_job_id(&job.id),
            job.status,
            repo_id
        )));
    }
    Ok(None)
}

fn hf_job_is_active(status: &str) -> bool {
    matches!(status, "queued" | "running" | "cancelling")
}

fn short_job_id(id: &str) -> String {
    id.chars().take(8).collect()
}

pub fn preflight_model(request: StartModelRequest) -> Result<ModelPreflightDto, String> {
    validate_repo_id(&request.repo_id)?;
    validate_local_server_settings(request.port, request.context)?;
    let model_path = find_model_file(&request.repo_id, request.filename.as_deref())?;
    let metadata = fs::metadata(&model_path).map_err(|err| err.to_string())?;
    let model_size_bytes = metadata.len();
    let selected_file = model_path
        .strip_prefix(model_dir(&request.repo_id))
        .unwrap_or(model_path.as_path())
        .to_string_lossy()
        .to_string();
    let estimated_memory_bytes = estimate_runtime_memory_bytes(model_size_bytes, request.context);
    let system_memory_bytes = system_memory_bytes();
    let findings = local_model_preflight_findings(
        &selected_file,
        model_size_bytes,
        request.context,
        system_memory_bytes,
    );

    let status = if !findings.errors.is_empty() {
        "error"
    } else if !findings.warnings.is_empty() {
        "warn"
    } else {
        "ok"
    }
    .to_string();
    let summary = format!(
        "{} selected; model {}, estimated runtime {}, system memory {}",
        selected_file,
        format_bytes_u64(model_size_bytes),
        format_bytes_u64(estimated_memory_bytes),
        system_memory_bytes
            .map(format_bytes_u64)
            .unwrap_or_else(|| "unknown".into())
    );
    Ok(ModelPreflightDto {
        status,
        summary,
        warnings: findings.warnings,
        errors: findings.errors,
        repo_id: request.repo_id,
        selected_file,
        model_size_bytes,
        estimated_memory_bytes,
        system_memory_bytes,
        context: request.context,
    })
}

pub fn start_server(
    state: &store::AppState,
    request: StartModelRequest,
) -> Result<HuggingFaceStatusDto, String> {
    start_server_with_cancel(state, request, || false)
}

fn start_server_with_cancel(
    state: &store::AppState,
    request: StartModelRequest,
    mut cancellation_requested: impl FnMut() -> bool,
) -> Result<HuggingFaceStatusDto, String> {
    if cancellation_requested() {
        return Err("cancelled".into());
    }
    if !adapters::command_exists("llama-server") {
        return Err("llama-server not found. Install with: brew install llama.cpp".into());
    }
    validate_repo_id(&request.repo_id)?;
    validate_local_server_settings(request.port, request.context)?;
    let model_path = find_model_file(&request.repo_id, request.filename.as_deref())?;
    let selected_file = model_path
        .strip_prefix(model_dir(&request.repo_id))
        .unwrap_or(model_path.as_path())
        .to_string_lossy()
        .to_string();
    validate_runnable_gguf_filename(&selected_file)?;
    enforce_start_preflight(&selected_file, &model_path, request.context)?;
    if cancellation_requested() {
        return Err("cancelled".into());
    }
    {
        let mut slot = state.hf_server.lock().map_err(|err| err.to_string())?;
        if let Some(mut running) = slot.take() {
            running.terminate();
        }
    }
    ensure_port_available(request.port)?;
    if cancellation_requested() {
        return Err("cancelled".into());
    }
    let log_path = paths::app_data_dir().join(format!("llama-server-{}.log", uuid::Uuid::new_v4()));
    fs::create_dir_all(paths::app_data_dir()).map_err(|err| err.to_string())?;
    let log_file = fs::File::create(&log_path).map_err(|err| {
        format!(
            "failed to create llama-server log at {}: {}",
            log_path.to_string_lossy(),
            err
        )
    })?;
    let log_file_err = log_file.try_clone().map_err(|err| err.to_string())?;
    let mut cmd = adapters::command_with_gui_path("llama-server");
    cmd.args([
        "-m",
        model_path.to_string_lossy().as_ref(),
        "--host",
        "127.0.0.1",
        "--port",
        &request.port.to_string(),
        "-c",
        &request.context.to_string(),
    ])
    .stdout(Stdio::from(log_file))
    .stderr(Stdio::from(log_file_err));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Some(token) = read_token() {
        cmd.env("HF_TOKEN", token);
    }
    let mut child = cmd.spawn().map_err(|err| err.to_string())?;
    let marker_path =
        store::write_hf_server_marker(child.id(), request.port, &model_path.to_string_lossy())
            .map_err(|err| err.to_string())?;
    if let Err(err) = wait_for_server_with_cancel(
        request.port,
        &mut child,
        &log_path,
        &mut cancellation_requested,
    ) {
        store::terminate_child_process(&mut child);
        let _ = fs::remove_file(&marker_path);
        return Err(err);
    }
    let server_model_id = served_model_id(request.port).ok().flatten();
    {
        let mut slot = state.hf_server.lock().map_err(|err| err.to_string())?;
        if let Some(mut running) = slot.take() {
            running.terminate();
        }
        *slot = Some(store::HfServerProcess {
            child,
            port: request.port,
            model_path: model_path.to_string_lossy().to_string(),
            server_model_id,
            marker_path: Some(marker_path),
        });
    }
    status(state)
}

pub fn stop_server(state: &store::AppState) -> Result<HuggingFaceStatusDto, String> {
    let mut slot = state.hf_server.lock().map_err(|err| err.to_string())?;
    if let Some(mut running) = slot.take() {
        running.terminate();
    }
    drop(slot);
    status(state)
}

fn read_token() -> Option<String> {
    if let Ok(token) = std::env::var("HF_TOKEN") {
        if !token.trim().is_empty() {
            return Some(token);
        }
    }
    if !cfg!(target_os = "macos") {
        return None;
    }
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            "HF_TOKEN",
            "-s",
            KEYCHAIN_SERVICE,
            "-w",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

fn downloaded_models() -> Result<Vec<DownloadedModelDto>, String> {
    let root = models_root();
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut models = Vec::new();
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(vec![]),
        Err(err) => return Err(err.to_string()),
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(err.to_string()),
        };
        if path.is_dir() {
            let repo_id = read_repo_id(&path).unwrap_or_else(|| {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .replace("__", "/")
            });
            match model_dto(&repo_id, &path) {
                Ok(model) => models.push(model),
                Err(err) if is_not_found_message(&err) => continue,
                Err(err) => return Err(err),
            }
        }
    }
    models.sort_by(|a, b| a.repo_id.cmp(&b.repo_id));
    Ok(models)
}

fn model_dto(repo_id: &str, path: &Path) -> Result<DownloadedModelDto, String> {
    let mut files = Vec::new();
    collect_files(path, path, &mut files)?;
    let gguf_files = files
        .iter()
        .filter(|file| file.to_lowercase().ends_with(".gguf"))
        .cloned()
        .collect::<Vec<_>>();
    let metadata = read_model_metadata(path).unwrap_or_default();
    let gguf_file_details = gguf_files
        .iter()
        .map(|file| gguf_file_detail(path, file, metadata.files.get(file)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DownloadedModelDto {
        repo_id: repo_id.into(),
        revision: metadata.revision,
        path: path.to_string_lossy().to_string(),
        files,
        gguf_files,
        gguf_file_details,
        size_bytes: directory_size(path)?,
        selected_file: metadata.selected_file,
        download_log: None,
    })
}

fn collect_files(root: &Path, current: &Path, files: &mut Vec<String>) -> Result<(), String> {
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.to_string()),
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(err.to_string()),
        };
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == ".cache" || name == ".benchforge-model.json")
        {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            files.push(relative.to_string_lossy().to_string());
        }
    }
    files.sort();
    Ok(())
}

fn directory_size(path: &Path) -> Result<u64, String> {
    let mut total = 0;
    if !path.exists() {
        return Ok(0);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.to_string()),
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(err.to_string()),
        };
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == ".cache" || name == ".benchforge-model.json")
        {
            continue;
        }
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(err.to_string()),
        };
        if metadata.is_dir() {
            total += directory_size(&path)?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn is_not_found_message(message: &str) -> bool {
    message.contains("No such file or directory") || message.contains("not found")
}

fn gguf_file_detail(
    root: &Path,
    file: &str,
    stored: Option<&StoredGgufFileMetadata>,
) -> Result<GgufFileDto, String> {
    let path = safety::safe_child_path(root, file)?;
    let size_bytes = fs::metadata(&path).map_err(|err| err.to_string())?.len();
    let sha256 = stored
        .filter(|metadata| metadata.size_bytes == Some(size_bytes))
        .and_then(|metadata| metadata.sha256.clone());
    let quantization = stored
        .and_then(|metadata| metadata.quantization.clone())
        .or_else(|| infer_gguf_quantization(file));
    Ok(GgufFileDto {
        file: file.into(),
        size_bytes,
        sha256,
        quantization,
    })
}

fn gguf_file_metadata(detail: &GgufFileDto) -> StoredGgufFileMetadata {
    StoredGgufFileMetadata {
        size_bytes: Some(detail.size_bytes),
        sha256: detail.sha256.clone(),
        quantization: detail.quantization.clone(),
    }
}

fn model_path_is_inside_dir(model_path: &str, dir: &Path) -> bool {
    let model_path = Path::new(model_path);
    if let (Ok(model_path), Ok(dir)) = (model_path.canonicalize(), dir.canonicalize()) {
        model_path.starts_with(dir)
    } else {
        model_path.starts_with(dir)
    }
}

struct LocalModelPreflightFindings {
    warnings: Vec<String>,
    errors: Vec<String>,
}

fn local_model_preflight_findings(
    selected_file: &str,
    model_size_bytes: u64,
    context: u32,
    system_memory_bytes: Option<u64>,
) -> LocalModelPreflightFindings {
    let estimated_memory_bytes = estimate_runtime_memory_bytes(model_size_bytes, context);
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    if is_projector_gguf(selected_file) {
        errors.push("Selected file looks like a multimodal projector, not the main text model. Choose the main model GGUF for local LLM benchmarking.".into());
    }
    if model_size_bytes < 64 * 1024 * 1024 {
        warnings.push("Selected GGUF is very small and may be a projector, tokenizer artifact, or smoke-test file rather than a useful LLM.".into());
    }
    if context > 8192 {
        warnings.push(format!(
            "Context {} is high for local CPU/Metal runs and can substantially increase memory use.",
            context
        ));
    }
    if context < 512 {
        warnings.push(
            "Context below 512 can make benchmark prompts fail from avoidable truncation.".into(),
        );
    }
    if !has_quantization_hint(selected_file) && model_size_bytes > 2 * 1024 * 1024 * 1024 {
        warnings.push(
            "Filename does not expose a common quantization marker such as Q4_K_M, Q5, Q8, or F16."
                .into(),
        );
    }
    if let Some(memory) = system_memory_bytes {
        if estimated_memory_bytes > memory {
            errors.push(format!(
                "Estimated runtime memory {} exceeds system memory {}.",
                format_bytes_u64(estimated_memory_bytes),
                format_bytes_u64(memory)
            ));
        } else if estimated_memory_bytes > memory.saturating_mul(85) / 100 {
            warnings.push(format!(
                "Estimated runtime memory {} is above 85% of system memory {}.",
                format_bytes_u64(estimated_memory_bytes),
                format_bytes_u64(memory)
            ));
        } else if estimated_memory_bytes > memory.saturating_mul(70) / 100 {
            warnings.push(format!(
                "Estimated runtime memory {} is above 70% of system memory {}.",
                format_bytes_u64(estimated_memory_bytes),
                format_bytes_u64(memory)
            ));
        }
    } else {
        warnings.push("Could not determine system memory for local model preflight.".into());
    }

    LocalModelPreflightFindings { warnings, errors }
}

fn enforce_start_preflight(
    selected_file: &str,
    model_path: &Path,
    context: u32,
) -> Result<(), String> {
    let model_size_bytes = fs::metadata(model_path)
        .map_err(|err| err.to_string())?
        .len();
    let findings = local_model_preflight_findings(
        selected_file,
        model_size_bytes,
        context,
        system_memory_bytes(),
    );
    if findings.errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "local model preflight failed: {}",
            findings.errors.join(" ")
        ))
    }
}

fn estimate_runtime_memory_bytes(model_size_bytes: u64, context: u32) -> u64 {
    let model_overhead = model_size_bytes.saturating_mul(13) / 10;
    let context_overhead = (context as u64)
        .saturating_mul(512 * 1024)
        .max(256 * 1024 * 1024);
    model_overhead
        .saturating_add(context_overhead)
        .saturating_add(512 * 1024 * 1024)
}

fn has_quantization_hint(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    [
        "q2_", "q3_", "q4_", "q5_", "q6_", "q8_", "iq", "f16", "bf16",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
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

fn system_memory_bytes() -> Option<u64> {
    if cfg!(target_os = "macos") {
        let output = Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    } else if cfg!(target_os = "linux") {
        let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
        meminfo.lines().find_map(|line| {
            let rest = line.strip_prefix("MemTotal:")?.trim();
            let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            Some(kb.saturating_mul(1024))
        })
    } else {
        None
    }
}

fn format_bytes_u64(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 || size >= 10.0 {
        format!("{:.0} {}", size, UNITS[unit])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

fn find_model_file(repo_id: &str, filename: Option<&str>) -> Result<PathBuf, String> {
    let dir = model_dir(repo_id);
    if !dir.exists() {
        return Err(format!("model {} is not downloaded yet", repo_id));
    }
    if let Some(filename) = filename.filter(|name| !name.trim().is_empty()) {
        let path = safety::safe_child_path(&dir, filename)?;
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("downloaded file not found: {}", filename));
    }
    let mut files = Vec::new();
    collect_files(&dir, &dir, &mut files)?;
    files
        .into_iter()
        .find(|file| file.ends_with(".gguf") && !file.to_lowercase().contains("mmproj"))
        .map(|file| dir.join(file))
        .ok_or_else(|| "no GGUF model file found; provide a GGUF filename".into())
}

fn validate_local_server_settings(port: u16, context: u32) -> Result<(), String> {
    if port < 1024 {
        return Err("Local model server port must be between 1024 and 65535.".into());
    }
    if !(128..=131_072).contains(&context) {
        return Err("Context must be between 128 and 131072 tokens.".into());
    }
    Ok(())
}

fn ensure_port_available(port: u16) -> Result<(), String> {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => {
            drop(listener);
            Ok(())
        }
        Err(err) => Err(format!(
            "Port {} is already in use on 127.0.0.1 ({}). Stop the other server or choose a different port before starting llama-server.",
            port, err
        )),
    }
}

fn wait_for_server_with_cancel(
    port: u16,
    child: &mut std::process::Child,
    log_path: &Path,
    cancellation_requested: &mut impl FnMut() -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        if cancellation_requested() {
            return Err("cancelled".into());
        }
        if get_url(&format!("http://127.0.0.1:{}/v1/models", port)).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            return Err(llama_startup_error(
                &format!(
                    "llama-server exited before becoming ready with status {}.",
                    status
                ),
                log_path,
            ));
        }
        thread::sleep(Duration::from_millis(750));
    }
    Err(llama_startup_error(
        "llama-server did not become ready within 90 seconds.",
        log_path,
    ))
}

fn served_model_id(port: u16) -> Result<Option<String>, String> {
    let body = get_url(&format!("http://127.0.0.1:{}/v1/models", port))?;
    Ok(first_openai_model_id(&body))
}

fn first_openai_model_id(body: &str) -> Option<String> {
    let json = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let items = json
        .get("data")
        .and_then(|value| value.as_array())
        .or_else(|| json.get("models").and_then(|value| value.as_array()))?;
    items.iter().find_map(|item| {
        item.get("id")
            .or_else(|| item.get("name"))
            .or_else(|| item.get("model"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
    })
}

fn llama_startup_error(prefix: &str, log_path: &Path) -> String {
    let log_tail = read_log_tail(log_path, 4000);
    if log_tail.is_empty() {
        format!(
            "{} No llama-server log output was captured. Log path: {}",
            prefix,
            log_path.to_string_lossy()
        )
    } else {
        format!(
            "{}\nllama-server log (tail):\n{}\nLog path: {}",
            prefix,
            log_tail,
            log_path.to_string_lossy()
        )
    }
}

fn read_log_tail(path: &Path, max_chars: usize) -> String {
    let Ok(raw) = fs::read_to_string(path) else {
        return String::new();
    };
    let text = strip_ansi_codes(raw).trim().to_string();
    if text.chars().count() <= max_chars {
        return text;
    }
    let tail = text
        .chars()
        .rev()
        .take(max_chars)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("...{}", tail)
}

fn get_url(url: &str) -> Result<String, String> {
    let output = adapters::command_with_gui_path("curl")
        .args(["-fsS", url])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn get_hf_api_url(url: &str) -> Result<String, String> {
    let mut cmd = adapters::command_with_gui_path("curl");
    cmd.args(["-fsSL", "-A", "BenchForge/0.1"]);
    if let Some(token) = read_token() {
        cmd.arg("-H")
            .arg(format!("Authorization: Bearer {}", token));
    }
    let output = cmd.arg(url).output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = strip_ansi_codes(String::from_utf8_lossy(&output.stderr));
        return Err(stderr.trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn hf_resolve_url(repo_id: &str, revision: Option<&str>, filename: &str) -> String {
    let revision = normalized_revision(revision);
    format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        url_encode_path(repo_id),
        url_encode_path(&revision),
        url_encode_path(filename)
    )
}

fn normalized_revision(revision: Option<&str>) -> String {
    revision
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("main")
        .to_string()
}

fn explicit_revision(revision: Option<&str>) -> Option<String> {
    revision
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn resolve_default_gguf_file(repo_id: &str, revision: Option<&str>) -> Result<String, String> {
    let files = resolve_repo_gguf_file_details(repo_id, revision)?
        .into_iter()
        .map(|detail| detail.file)
        .collect::<Vec<_>>();
    recommend_gguf_file(&files).ok_or_else(|| {
        let visible_files = format_visible_file_list(&files);
        format!(
            "{} has no runnable text GGUF files visible through the Hugging Face API. {} Select an exact .gguf file only if it is the main model, not an mmproj/projector file.",
            repo_id, visible_files
        )
    })
}

fn resolve_repo_gguf_file_details(
    repo_id: &str,
    revision: Option<&str>,
) -> Result<Vec<GgufFileDto>, String> {
    let revision = normalized_revision(revision);
    let tree_url = hf_model_tree_url(repo_id, Some(&revision));
    match get_hf_api_url(&tree_url)
        .and_then(|raw| parse_hf_json(&raw, "Hugging Face model tree response"))
    {
        Ok(json) => {
            let details = gguf_file_details_from_tree_value(&json);
            if !details.is_empty() {
                return Ok(details);
            }
        }
        Err(tree_err) => {
            let model_details =
                resolve_repo_gguf_file_details_from_model_api(repo_id, Some(&revision))?;
            if !model_details.is_empty() {
                return Ok(model_details);
            }
            return Err(tree_err);
        }
    }
    resolve_repo_gguf_file_details_from_model_api(repo_id, Some(&revision))
}

fn resolve_repo_gguf_file_details_from_model_api(
    repo_id: &str,
    revision: Option<&str>,
) -> Result<Vec<GgufFileDto>, String> {
    let url = hf_model_api_url(repo_id, revision);
    let raw = get_hf_api_url(&url)?;
    let json = parse_hf_json(&raw, "Hugging Face model response")?;
    Ok(gguf_file_details_from_value(&json))
}

fn hf_model_tree_url(repo_id: &str, revision: Option<&str>) -> String {
    format!(
        "https://huggingface.co/api/models/{}/tree/{}?recursive=true",
        url_encode_path(repo_id),
        url_encode_path(&normalized_revision(revision))
    )
}

fn hf_model_api_url(repo_id: &str, revision: Option<&str>) -> String {
    format!(
        "https://huggingface.co/api/models/{}?full=true&revision={}",
        url_encode_path(repo_id),
        url_encode(&normalized_revision(revision))
    )
}

fn parse_hf_json(raw: &str, label: &str) -> Result<serde_json::Value, String> {
    let json: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| format!("invalid {}: {}", label, err))?;
    if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
        return Err(error.to_string());
    }
    Ok(json)
}

fn prepare_download(request: &DownloadModelRequest) -> Result<PreparedDownload, String> {
    validate_repo_id(&request.repo_id)?;
    let revision = explicit_revision(request.revision.as_deref());
    let selected_file = match normalized_optional_filename(request.filename.as_deref())? {
        Some(filename) => filename,
        None => resolve_default_gguf_file(&request.repo_id, revision.as_deref())?,
    };
    validate_runnable_gguf_filename(&selected_file)?;
    let model_dir = model_dir(&request.repo_id);
    fs::create_dir_all(&model_dir).map_err(|err| err.to_string())?;
    let hf_home_dir = download_hf_home_dir(&model_dir);
    let plan = if adapters::command_exists("hf") {
        match dry_run_download(
            &request.repo_id,
            Some(selected_file.as_str()),
            revision.as_deref(),
            &hf_home_dir,
        ) {
            Ok(plan) => plan,
            Err(err) if adapters::command_exists("curl") => direct_download_plan(
                &request.repo_id,
                &selected_file,
                revision.as_deref(),
                Some(&err),
            ),
            Err(err) => return Err(err),
        }
    } else if adapters::command_exists("curl") {
        direct_download_plan(&request.repo_id, &selected_file, revision.as_deref(), None)
    } else {
        return Err(
            "Neither hf CLI nor curl was found. Install Hugging Face CLI with: curl -LsSf https://hf.co/cli/install.sh | bash -s"
                .into(),
        );
    };
    let disk_space_log = check_download_disk_space(&model_dir, plan.planned_bytes)?;
    let expected_file = safety::safe_child_path(&model_dir, &selected_file)?;
    let expected_sha256 =
        remote_expected_sha256(&request.repo_id, revision.as_deref(), &selected_file);
    let existing_bytes = file_size(&expected_file);
    let partial_bytes = partial_download_bytes(&model_dir);
    let existing_validation = validate_existing_download_reuse(
        &model_dir,
        &selected_file,
        &expected_file,
        existing_bytes,
        plan.planned_bytes,
        expected_sha256.as_deref(),
    );
    let already_downloaded = existing_validation.reusable;
    Ok(PreparedDownload {
        selected_file,
        revision,
        model_dir,
        hf_home_dir,
        expected_file,
        expected_sha256,
        plan,
        disk_space_log,
        existing_bytes,
        partial_bytes,
        already_downloaded,
        existing_integrity_log: existing_validation.log,
        remove_existing_before_download: existing_validation.remove_before_download,
    })
}

fn prepared_download_dto(repo_id: &str, prepared: &PreparedDownload) -> DownloadModelPlanDto {
    DownloadModelPlanDto {
        repo_id: repo_id.into(),
        selected_file: prepared.selected_file.clone(),
        revision: prepared.revision.clone(),
        local_dir: prepared.model_dir.to_string_lossy().to_string(),
        planned_bytes: prepared.plan.planned_bytes,
        existing_bytes: prepared.existing_bytes,
        partial_bytes: prepared.partial_bytes,
        already_downloaded: prepared.already_downloaded,
        summary: prepared.plan.summary.clone(),
        disk_check: prepared.disk_space_log.clone(),
        retry_hint: download_retry_hint(prepared),
    }
}

fn existing_download_matches_plan(existing_bytes: Option<u64>, planned_bytes: Option<u64>) -> bool {
    let Some(existing_bytes) = existing_bytes else {
        return false;
    };
    let Some(planned_bytes) = planned_bytes else {
        return false;
    };
    planned_bytes > 0 && existing_bytes >= planned_bytes
}

fn download_destination_summary(prepared: &PreparedDownload) -> String {
    let mut parts = vec![format!(
        "Download target: {}",
        prepared.expected_file.to_string_lossy()
    )];
    parts.push(format!(
        "Hugging Face cache: {}.",
        prepared.hf_home_dir.to_string_lossy()
    ));
    if let Some(revision) = &prepared.revision {
        parts.push(format!("Hugging Face revision: {}.", revision));
    }
    if let Some(existing_bytes) = prepared.existing_bytes {
        parts.push(format!(
            "Existing local file before download: {}.",
            format_bytes_u64(existing_bytes)
        ));
    }
    if let Some(log) = &prepared.existing_integrity_log {
        parts.push(log.clone());
    }
    if let Some(expected_sha256) = &prepared.expected_sha256 {
        parts.push(format!(
            "Expected Hugging Face SHA-256: {}.",
            short_hash(expected_sha256)
        ));
    }
    if prepared.partial_bytes > 0 {
        parts.push(format!(
            "Partial/cache fragments detected before download: {}.",
            format_bytes_u64(prepared.partial_bytes)
        ));
    }
    parts.join("\n")
}

fn download_retry_hint(prepared: &PreparedDownload) -> String {
    if prepared.already_downloaded {
        return "File already matches the planned download; BenchForge can use it without another network transfer.".into();
    }
    if prepared.remove_existing_before_download {
        return "Retry: the existing GGUF matched the expected size but failed saved SHA-256 validation. BenchForge will remove it and download a clean copy.".into();
    }
    if prepared.partial_bytes > 0 {
        return format!(
            "Retry: {} of partial Hugging Face download data was found. Click Download again to let hf or curl reuse local bytes where possible, or delete the model files to restart cleanly.",
            format_bytes_u64(prepared.partial_bytes)
        );
    }
    "Retry: click Download again after fixing the issue. BenchForge reuses hf cache state or existing local file bytes where possible.".into()
}

fn validate_existing_download_reuse(
    model_dir: &Path,
    selected_file: &str,
    expected_file: &Path,
    existing_bytes: Option<u64>,
    planned_bytes: Option<u64>,
    remote_expected_sha256: Option<&str>,
) -> ExistingDownloadValidation {
    if !existing_download_matches_plan(existing_bytes, planned_bytes) {
        return ExistingDownloadValidation::default();
    }

    let Some(existing_bytes) = existing_bytes else {
        return ExistingDownloadValidation::default();
    };
    if let Some(expected_sha) = remote_expected_sha256.filter(|value| !value.trim().is_empty()) {
        return validate_existing_file_against_sha(
            expected_file,
            expected_sha,
            "Hugging Face",
            true,
        );
    }

    let Some(stored) = read_model_metadata(model_dir)
        .and_then(|metadata| metadata.files.get(selected_file).cloned())
    else {
        return ExistingDownloadValidation {
            reusable: true,
            log: Some(
                "Existing file matches the planned size; no saved SHA-256 was available to verify it.".into(),
            ),
            remove_before_download: false,
        };
    };
    if stored
        .size_bytes
        .is_some_and(|stored_size| stored_size != existing_bytes)
    {
        return ExistingDownloadValidation {
            reusable: true,
            log: Some(
                "Existing file matches the planned size; saved SHA-256 was not checked because stored metadata size differs.".into(),
            ),
            remove_before_download: false,
        };
    }
    let Some(expected_sha) = stored.sha256.filter(|value| !value.trim().is_empty()) else {
        return ExistingDownloadValidation {
            reusable: true,
            log: Some(
                "Existing file matches the planned size; saved metadata has no SHA-256 to verify."
                    .into(),
            ),
            remove_before_download: false,
        };
    };

    validate_existing_file_against_sha(expected_file, &expected_sha, "saved", true)
}

fn validate_existing_file_against_sha(
    expected_file: &Path,
    expected_sha: &str,
    source: &str,
    remove_before_download: bool,
) -> ExistingDownloadValidation {
    match checksum_file(expected_file) {
        Ok(actual_sha) if actual_sha == expected_sha => ExistingDownloadValidation {
            reusable: true,
            log: Some(format!(
                "Existing file matches the planned size and {} SHA-256.",
                source
            )),
            remove_before_download: false,
        },
        Ok(actual_sha) => ExistingDownloadValidation {
            reusable: false,
            log: Some(format!(
                "Existing file matches the planned size but failed {} SHA-256 validation (expected {}, got {}); BenchForge will re-download it.",
                source,
                short_hash(expected_sha),
                short_hash(&actual_sha)
            )),
            remove_before_download,
        },
        Err(err) => ExistingDownloadValidation {
            reusable: false,
            log: Some(format!(
                "Existing file matches the planned size but could not be hashed for {} SHA-256 validation: {}; BenchForge will re-download it.",
                source,
                err
            )),
            remove_before_download,
        },
    }
}

fn short_hash(value: &str) -> String {
    value.chars().take(12).collect()
}

fn dry_run_download(
    repo_id: &str,
    filename: Option<&str>,
    revision: Option<&str>,
    hf_home_dir: &Path,
) -> Result<DownloadPlan, String> {
    let mut args = vec!["download".to_string(), repo_id.to_string()];
    if let Some(filename) = filename {
        args.push(filename.to_string());
    }
    if let Some(revision) = revision.filter(|value| !value.trim().is_empty()) {
        args.push("--revision".into());
        args.push(revision.to_string());
    }
    args.extend(["--dry-run".into(), "--json".into()]);

    let mut cmd = adapters::command_with_gui_path("hf");
    cmd.args(args).env("HF_HOME", hf_home_dir);
    if let Some(token) = read_token() {
        cmd.env("HF_TOKEN", token);
    }
    let output = cmd.output().map_err(|err| err.to_string())?;
    let log = command_log(&output.stdout, &output.stderr);
    if !output.status.success() {
        return Err(friendly_download_error(&log));
    }
    Ok(parse_dry_run_plan(&output.stdout).unwrap_or(DownloadPlan {
        summary: log,
        planned_bytes: None,
    }))
}

fn direct_download_plan(
    repo_id: &str,
    selected_file: &str,
    revision: Option<&str>,
    hf_dry_run_error: Option<&str>,
) -> DownloadPlan {
    let details = resolve_repo_gguf_file_details(repo_id, revision).unwrap_or_default();
    direct_download_plan_from_details(selected_file, &details, hf_dry_run_error)
}

fn direct_download_plan_from_details(
    selected_file: &str,
    details: &[GgufFileDto],
    hf_dry_run_error: Option<&str>,
) -> DownloadPlan {
    let planned_bytes = details
        .iter()
        .find(|detail| detail.file == selected_file)
        .and_then(|detail| (detail.size_bytes > 0).then_some(detail.size_bytes));
    let size = planned_bytes
        .map(format_bytes_u64)
        .unwrap_or_else(|| "size unknown".into());
    let mut summary = format!("Planned direct download: {} ({})", selected_file, size);
    if let Some(error) = hf_dry_run_error.and_then(first_non_empty_line) {
        summary.push_str(&format!(
            ". hf CLI dry-run was unavailable, so BenchForge will use curl: {}",
            error
        ));
    } else {
        summary.push_str(". BenchForge will use curl because the hf CLI is unavailable.");
    }
    DownloadPlan {
        summary,
        planned_bytes,
    }
}

fn first_non_empty_line(input: &str) -> Option<&str> {
    input.lines().map(str::trim).find(|line| !line.is_empty())
}

fn parse_dry_run_plan(stdout: &[u8]) -> Option<DownloadPlan> {
    let text = String::from_utf8_lossy(stdout);
    let mut json = None;
    for (index, _) in text.match_indices('[') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text[index..]) {
            json = Some(value);
            break;
        }
    }
    let json = json?;
    let items = json.as_array()?;
    if items.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    let mut total_bytes = 0_u64;
    let mut all_sizes_known = true;
    for item in items.iter().take(5) {
        let file = item
            .get("file")
            .and_then(|value| value.as_str())
            .unwrap_or("file");
        let size = item
            .get("size")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown size");
        if let Some(bytes) = parse_hf_size_bytes(size) {
            total_bytes = total_bytes.saturating_add(bytes);
        } else {
            all_sizes_known = false;
        }
        parts.push(format!("{} ({})", file, size));
    }
    for item in items.iter().skip(5) {
        let Some(size) = item.get("size").and_then(|value| value.as_str()) else {
            all_sizes_known = false;
            continue;
        };
        if let Some(bytes) = parse_hf_size_bytes(size) {
            total_bytes = total_bytes.saturating_add(bytes);
        } else {
            all_sizes_known = false;
        }
    }
    let suffix = if items.len() > parts.len() {
        format!(" and {} more", items.len() - parts.len())
    } else {
        String::new()
    };
    Some(DownloadPlan {
        summary: format!("Planned download: {}{}", parts.join(", "), suffix),
        planned_bytes: if all_sizes_known {
            Some(total_bytes)
        } else {
            None
        },
    })
}

fn parse_hf_size_bytes(input: &str) -> Option<u64> {
    let normalized = input.trim().replace(',', "");
    let mut split_at = normalized.len();
    for (index, ch) in normalized.char_indices() {
        if !(ch.is_ascii_digit() || ch == '.') {
            split_at = index;
            break;
        }
    }
    let value = normalized[..split_at].trim().parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let unit = normalized[split_at..]
        .trim()
        .trim_end_matches('s')
        .to_lowercase()
        .replace(' ', "");
    let multiplier = match unit.as_str() {
        "" | "b" | "byte" => 1_f64,
        "k" | "kb" => 1000_f64,
        "m" | "mb" => 1000_f64.powi(2),
        "g" | "gb" => 1000_f64.powi(3),
        "t" | "tb" => 1000_f64.powi(4),
        "ki" | "kib" => 1024_f64,
        "mi" | "mib" => 1024_f64.powi(2),
        "gi" | "gib" => 1024_f64.powi(3),
        "ti" | "tib" => 1024_f64.powi(4),
        _ => return None,
    };
    Some((value * multiplier).round() as u64)
}

fn check_download_disk_space(dir: &Path, planned_bytes: Option<u64>) -> Result<String, String> {
    let Some(planned_bytes) = planned_bytes else {
        return Ok("Disk check skipped: planned download size is unknown.".into());
    };
    let Some(available_bytes) = available_disk_bytes(dir) else {
        return Ok("Disk check skipped: free space could not be determined.".into());
    };
    if planned_bytes > available_bytes {
        return Err(format!(
            "Not enough disk space for download. Planned {}, available {} at {}.",
            format_bytes_u64(planned_bytes),
            format_bytes_u64(available_bytes),
            dir.to_string_lossy()
        ));
    }
    let remaining = available_bytes.saturating_sub(planned_bytes);
    if remaining < DISK_SPACE_WARNING_BYTES {
        return Ok(format!(
            "Disk check warning: planned {}, available {}, leaving about {} free.",
            format_bytes_u64(planned_bytes),
            format_bytes_u64(available_bytes),
            format_bytes_u64(remaining)
        ));
    }
    Ok(format!(
        "Disk check passed: planned {}, available {}.",
        format_bytes_u64(planned_bytes),
        format_bytes_u64(available_bytes)
    ))
}

fn available_disk_bytes(dir: &Path) -> Option<u64> {
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

fn file_size(path: &Path) -> Option<u64> {
    path.metadata()
        .ok()
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
}

fn partial_download_bytes(dir: &Path) -> u64 {
    let mut total = 0_u64;
    collect_partial_download_bytes(dir, 0, &mut total);
    total
}

fn download_hf_home_dir(model_dir: &Path) -> PathBuf {
    model_dir.join(".cache").join("hf-home")
}

fn collect_partial_download_bytes(dir: &Path, depth: usize, total: &mut u64) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect_partial_download_bytes(&path, depth + 1, total);
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".incomplete") || name.ends_with(".part") || name.contains(".incomplete.")
        {
            *total = total.saturating_add(metadata.len());
        }
    }
}

fn friendly_download_error(log: &str) -> String {
    let lower = log.to_lowercase();
    let mut hints = Vec::new();
    if lower.contains("gated")
        || lower.contains("private")
        || lower.contains("repository not found")
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("authentication")
        || lower.contains("authenticated")
    {
        if read_token().is_some() {
            hints.push("Confirm the saved Hugging Face token has access to this repo and accept any model license on the Hugging Face website.");
        } else {
            hints.push("If this is a gated or private model, save an hf_ token in BenchForge first and accept any model license on the Hugging Face website.");
        }
    }
    if lower.contains("no such file")
        || lower.contains("entry not found")
        || lower.contains("404 client error")
    {
        hints.push(
            "Select an exact .gguf filename from the Hub model browser or the repo file tree.",
        );
    }
    if hints.is_empty() {
        log.to_string()
    } else {
        join_non_empty(&[log.to_string(), format!("Hint: {}", hints.join(" "))])
    }
}

fn command_log(stdout: &[u8], stderr: &[u8]) -> String {
    join_non_empty(&[
        strip_ansi_codes(String::from_utf8_lossy(stdout).trim()),
        strip_ansi_codes(String::from_utf8_lossy(stderr).trim()),
    ])
}

fn join_non_empty(parts: &[String]) -> String {
    let text = parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        "command completed without output".into()
    } else {
        text
    }
}

fn normalize_model_sort(sort: &str) -> &'static str {
    match sort {
        "downloads" => "downloads",
        "likes" => "likes",
        "lastModified" | "recent" => "lastModified",
        "trending" | "trendingScore" | _ => "trendingScore",
    }
}

fn model_from_value(item: &serde_json::Value) -> Option<HuggingFaceModelDto> {
    let repo_id = item
        .get("id")
        .or_else(|| item.get("modelId"))?
        .as_str()?
        .to_string();
    let tags = item
        .get("tags")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let gguf_files = gguf_files_from_value(item);
    let recommended_file = recommend_gguf_file(&gguf_files);
    Some(HuggingFaceModelDto {
        author: item
            .get("author")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        url: format!("https://huggingface.co/{}", repo_id),
        downloads: item
            .get("downloads")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        likes: item
            .get("likes")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        trending_score: item.get("trendingScore").and_then(|value| value.as_f64()),
        pipeline_tag: item
            .get("pipeline_tag")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        library_name: item
            .get("library_name")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        gated: is_gated(item.get("gated")),
        last_modified: item
            .get("lastModified")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        tags,
        gguf_files,
        recommended_file,
        repo_id,
    })
}

fn gguf_files_from_value(item: &serde_json::Value) -> Vec<String> {
    gguf_file_details_from_value(item)
        .into_iter()
        .map(|detail| detail.file)
        .collect()
}

fn gguf_file_details_from_value(item: &serde_json::Value) -> Vec<GgufFileDto> {
    let mut details = item
        .get("siblings")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|value| {
                    let filename = value
                        .get("rfilename")
                        .or_else(|| value.get("path"))
                        .and_then(|filename| filename.as_str())?;
                    filename.to_lowercase().ends_with(".gguf").then(|| {
                        remote_gguf_file_detail(
                            filename,
                            remote_file_size(value),
                            remote_file_sha256(value),
                        )
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    details.sort_by(|a, b| a.file.cmp(&b.file));
    details
}

fn gguf_file_details_from_tree_value(item: &serde_json::Value) -> Vec<GgufFileDto> {
    let mut details = item
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry
                .get("type")
                .and_then(|value| value.as_str())
                .map(|kind| kind == "file")
                .unwrap_or(true)
        })
        .filter_map(|entry| {
            let filename = entry
                .get("path")
                .or_else(|| entry.get("rfilename"))
                .and_then(|filename| filename.as_str())?;
            filename.to_lowercase().ends_with(".gguf").then(|| {
                remote_gguf_file_detail(
                    filename,
                    remote_file_size(entry),
                    remote_file_sha256(entry),
                )
            })
        })
        .collect::<Vec<_>>();
    details.sort_by(|a, b| a.file.cmp(&b.file));
    details
}

fn remote_gguf_file_detail(file: &str, size_bytes: u64, sha256: Option<String>) -> GgufFileDto {
    GgufFileDto {
        file: file.into(),
        size_bytes,
        sha256,
        quantization: infer_gguf_quantization(file),
    }
}

fn remote_file_size(entry: &serde_json::Value) -> u64 {
    entry
        .get("lfs")
        .and_then(|lfs| lfs.get("size"))
        .and_then(|value| value.as_u64())
        .or_else(|| entry.get("size").and_then(|value| value.as_u64()))
        .unwrap_or(0)
}

fn remote_file_sha256(entry: &serde_json::Value) -> Option<String> {
    [
        entry
            .get("lfs")
            .and_then(|lfs| lfs.get("oid"))
            .and_then(|value| value.as_str()),
        entry
            .get("lfs")
            .and_then(|lfs| lfs.get("sha256"))
            .and_then(|value| value.as_str()),
        entry.get("oid").and_then(|value| value.as_str()),
        entry.get("sha256").and_then(|value| value.as_str()),
    ]
    .into_iter()
    .flatten()
    .find_map(normalize_sha256)
}

fn remote_expected_sha256(
    repo_id: &str,
    revision: Option<&str>,
    selected_file: &str,
) -> Option<String> {
    resolve_repo_gguf_file_details(repo_id, revision)
        .ok()?
        .into_iter()
        .find(|detail| detail.file == selected_file)?
        .sha256
}

fn normalize_sha256(value: &str) -> Option<String> {
    let trimmed = value.trim().to_lowercase();
    (trimmed.len() == 64 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit())).then_some(trimmed)
}

fn recommend_gguf_file(files: &[String]) -> Option<String> {
    let runnable = files
        .iter()
        .filter(|file| !is_projector_gguf(file))
        .collect::<Vec<_>>();
    let preferred = runnable
        .iter()
        .copied()
        .filter(|file| !is_low_priority_runtime_file(file))
        .collect::<Vec<_>>();
    ranked_gguf_file(&preferred).or_else(|| ranked_gguf_file(&runnable))
}

fn ranked_gguf_file(candidates: &[&String]) -> Option<String> {
    for pattern in [
        "ud-q4_k_m",
        "q4_k_m",
        "ud-q4_k_s",
        "q4_k_s",
        "q4_0",
        "iq4_xs",
        "iq4_nl",
        "q5_k_m",
        "q5_k_s",
        "q6_k",
        "q8_0",
    ] {
        if let Some(file) = candidates
            .iter()
            .find(|file| file.to_lowercase().contains(pattern))
        {
            return Some((**file).clone());
        }
    }
    candidates.first().map(|file| (**file).clone())
}

fn format_visible_file_list(files: &[String]) -> String {
    if files.is_empty() {
        return "No .gguf files were listed.".into();
    }
    let mut listed = files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    if files.len() > 5 {
        listed.push_str(&format!(", and {} more", files.len() - 5));
    }
    format!("Visible GGUF files: {}.", listed)
}

fn validate_runnable_gguf_filename(filename: &str) -> Result<(), String> {
    if is_projector_gguf(filename) {
        return Err(
            "selected GGUF is a multimodal projector, not a runnable text model; choose the main model GGUF file"
                .into(),
        );
    }
    Ok(())
}

fn is_projector_gguf(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.contains("mmproj") || lower.contains("projector")
}

fn is_low_priority_runtime_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.contains("/bf16/") || lower.contains("f16")
}

fn is_gated(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(flag)) => *flag,
        Some(serde_json::Value::String(text)) => text != "false",
        _ => false,
    }
}

fn url_encode(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            other => format!("%{:02X}", other).chars().collect(),
        })
        .collect()
}

fn url_encode_path(path: &str) -> String {
    path.split('/')
        .map(|part| url_encode(part).replace('+', "%20"))
        .collect::<Vec<_>>()
        .join("/")
}

fn ensure_supported_python(log: &mut Vec<String>) -> Result<(), String> {
    if let Some(version) = python3_version() {
        if version_is_at_least(&version, 3, 10) {
            log.push(format!(
                "python3 {} is ready for the Hugging Face installer",
                version
            ));
            return Ok(());
        }
        log.push(format!(
            "python3 {} is too old for the Hugging Face installer; Python 3.10+ is required",
            version
        ));
    } else {
        log.push(
            "python3 was not found; Python 3.10+ is required for the Hugging Face installer"
                .to_string(),
        );
    }

    if !adapters::command_exists("brew") {
        log.push("Homebrew is required to install Python automatically".to_string());
        return Err(log.join("\n"));
    }

    let mut cmd = adapters::command_with_gui_path("brew");
    cmd.args(["install", "python"]);
    run_install_command("Python 3.10+", &mut cmd, log)?;

    if let Some(version) = python3_version() {
        if version_is_at_least(&version, 3, 10) {
            log.push(format!(
                "python3 {} is ready for the Hugging Face installer",
                version
            ));
            return Ok(());
        }
        log.push(format!(
            "python3 is still {} after installing Python",
            version
        ));
    }

    Err(log.join("\n"))
}

fn python3_version() -> Option<String> {
    let output = adapters::command_with_gui_path("python3")
        .arg("--version")
        .output()
        .ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.split_whitespace()
        .find(|part| {
            part.chars().next().is_some_and(|ch| ch.is_ascii_digit()) && part.contains('.')
        })
        .map(|part| part.trim().to_string())
}

fn version_is_at_least(version: &str, major: u32, minor: u32) -> bool {
    let mut parts = version.split('.');
    let found_major = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    let found_minor = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0);
    (found_major, found_minor) >= (major, minor)
}

fn run_install_command(
    label: &str,
    cmd: &mut Command,
    log: &mut Vec<String>,
) -> Result<(), String> {
    log.push(format!("Installing {}...", label));
    let output = cmd
        .output()
        .map_err(|err| format!("failed to start {} installer: {}", label, err))?;
    append_install_stream(log, label, "stdout", &output.stdout);
    append_install_stream(log, label, "stderr", &output.stderr);
    if !output.status.success() {
        return Err(format!(
            "{} install failed with exit code {:?}\n{}",
            label,
            output.status.code(),
            log.join("\n")
        ));
    }
    log.push(format!("{} installed", label));
    Ok(())
}

fn append_install_stream(log: &mut Vec<String>, label: &str, stream: &str, bytes: &[u8]) {
    let text = strip_ansi_codes(String::from_utf8_lossy(bytes).trim());
    if text.is_empty() {
        return;
    }
    let clipped: String = text.chars().take(6000).collect();
    let suffix = if text.chars().count() > clipped.chars().count() {
        "\n... truncated ..."
    } else {
        ""
    };
    log.push(format!("{} {}:\n{}{}", label, stream, clipped, suffix));
}

fn strip_ansi_codes(input: impl AsRef<str>) -> String {
    let mut output = String::new();
    let mut chars = input.as_ref().chars().peekable();
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

fn write_metadata(
    repo_id: &str,
    dir: &Path,
    revision: Option<&str>,
    selected_file: Option<&str>,
    file_detail: Option<&GgufFileDto>,
) -> Result<(), String> {
    let mut metadata = read_model_metadata(dir).unwrap_or_default();
    metadata.repo_id = Some(repo_id.into());
    metadata.revision = explicit_revision(revision);
    if let Some(selected_file) = selected_file {
        metadata.selected_file = Some(selected_file.into());
    }
    if let Some(file_detail) = file_detail {
        metadata
            .files
            .insert(file_detail.file.clone(), gguf_file_metadata(file_detail));
    }
    fs::write(
        dir.join(".benchforge-model.json"),
        serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

fn read_model_metadata(dir: &Path) -> Option<ModelMetadata> {
    let raw = fs::read_to_string(dir.join(".benchforge-model.json")).ok()?;
    serde_json::from_str::<ModelMetadata>(&raw).ok()
}

fn read_repo_id(dir: &Path) -> Option<String> {
    read_model_metadata(dir)?.repo_id
}

fn model_dir(repo_id: &str) -> PathBuf {
    models_root().join(sanitize_repo_id(repo_id))
}

fn models_root() -> PathBuf {
    paths::app_data_dir().join("models")
}

fn sanitize_repo_id(repo_id: &str) -> String {
    repo_id.replace('/', "__")
}

fn validate_repo_id(repo_id: &str) -> Result<(), String> {
    if repo_id.trim().is_empty() || repo_id.contains("..") || repo_id.starts_with('/') {
        Err("invalid Hugging Face repo id".into())
    } else {
        Ok(())
    }
}

fn normalized_optional_filename(filename: Option<&str>) -> Result<Option<String>, String> {
    let Some(filename) = filename.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = filename.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("invalid Hugging Face filename".into());
    }
    if !normalized.to_lowercase().ends_with(".gguf") {
        return Err("select a .gguf file to download and run locally".into());
    }
    Ok(Some(normalized))
}

fn checksum_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_id_sanitizes_for_local_dir() {
        assert_eq!(sanitize_repo_id("org/model-GGUF"), "org__model-GGUF");
    }

    #[test]
    fn rejects_bad_repo_id() {
        assert!(validate_repo_id("../secret").is_err());
        assert!(validate_repo_id("Qwen/Qwen2.5-0.5B-Instruct-GGUF").is_ok());
    }

    #[test]
    fn python_version_comparison_handles_minor_versions() {
        assert!(version_is_at_least("3.14.6", 3, 10));
        assert!(version_is_at_least("3.10.0", 3, 10));
        assert!(!version_is_at_least("3.9.6", 3, 10));
    }

    #[test]
    fn installer_log_strips_ansi_codes() {
        assert_eq!(
            strip_ansi_codes("\u{1b}[0;34m[INFO]\u{1b}[0m Done"),
            "[INFO] Done"
        );
    }

    #[test]
    fn model_search_helpers_match_hub_api() {
        assert_eq!(normalize_model_sort("trending"), "trendingScore");
        assert_eq!(normalize_model_sort("downloads"), "downloads");
        assert_eq!(url_encode("Qwen 3.6/GGUF"), "Qwen+3.6%2FGGUF");
        assert_eq!(url_encode_path("org/model name"), "org/model%20name");
        assert_eq!(normalized_revision(None), "main");
        assert_eq!(normalized_revision(Some(" refs/pr/1 ")), "refs/pr/1");
        assert_eq!(
            hf_model_tree_url("org name/model GGUF", Some("refs/pr/1")),
            "https://huggingface.co/api/models/org%20name/model%20GGUF/tree/refs/pr/1?recursive=true"
        );
        assert_eq!(
            hf_model_api_url("org name/model GGUF", Some("release candidate")),
            "https://huggingface.co/api/models/org%20name/model%20GGUF?full=true&revision=release+candidate"
        );
    }

    #[test]
    fn recommended_file_prefers_q4_non_projector() {
        let files = vec![
            "mmproj-F16.gguf".to_string(),
            "Model-Q8_0.gguf".to_string(),
            "Model-Q4_K_M.gguf".to_string(),
        ];
        assert_eq!(
            recommend_gguf_file(&files),
            Some("Model-Q4_K_M.gguf".into())
        );
    }

    #[test]
    fn recommended_file_falls_back_to_full_precision_text_gguf() {
        let files = vec![
            "mmproj-model-F16.gguf".to_string(),
            "Model-BF16.gguf".to_string(),
        ];
        assert_eq!(recommend_gguf_file(&files), Some("Model-BF16.gguf".into()));
    }

    #[test]
    fn visible_file_list_explains_projector_only_repos() {
        let files = vec!["mmproj-model-F16.gguf".to_string()];
        let detail = format_visible_file_list(&files);
        assert!(detail.contains("Visible GGUF files"));
        assert!(detail.contains("mmproj-model-F16.gguf"));
    }

    #[test]
    fn tree_api_parser_finds_nested_gguf_files() {
        let tree = serde_json::json!([
            {"type": "file", "path": "README.md"},
            {"type": "directory", "path": "BF16"},
            {"type": "file", "path": "BF16/Model-BF16-00001-of-00002.gguf"},
            {"type": "file", "path": "Model-Q4_K_M.gguf"},
            {"type": "file", "path": "mmproj-F16.gguf"}
        ]);
        assert_eq!(
            gguf_file_details_from_tree_value(&tree)
                .into_iter()
                .map(|detail| detail.file)
                .collect::<Vec<_>>(),
            vec![
                "BF16/Model-BF16-00001-of-00002.gguf".to_string(),
                "Model-Q4_K_M.gguf".to_string(),
                "mmproj-F16.gguf".to_string(),
            ]
        );
    }

    #[test]
    fn tree_api_parser_returns_file_details_with_sizes() {
        let tree = serde_json::json!([
            {"type": "file", "path": "Model-Q4_K_M.gguf", "lfs": {"size": 4096, "oid": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},
            {"type": "file", "path": "nested/Model-Q8_0.gguf", "size": 8192},
            {"type": "file", "path": "README.md", "size": 12}
        ]);

        let details = gguf_file_details_from_tree_value(&tree);

        assert_eq!(details.len(), 2);
        assert_eq!(details[0].file, "Model-Q4_K_M.gguf");
        assert_eq!(details[0].size_bytes, 4096);
        assert_eq!(
            details[0].sha256.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(details[0].quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(details[1].file, "nested/Model-Q8_0.gguf");
        assert_eq!(details[1].size_bytes, 8192);
        assert_eq!(details[1].quantization.as_deref(), Some("Q8_0"));
    }

    #[test]
    fn model_api_parser_returns_sibling_file_details() {
        let model = serde_json::json!({
            "siblings": [
                {"rfilename": "README.md"},
                {"rfilename": "Model-Q5_K_M.gguf", "size": 16384, "lfs": {"oid": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}
            ]
        });

        let details = gguf_file_details_from_value(&model);

        assert_eq!(details.len(), 1);
        assert_eq!(details[0].file, "Model-Q5_K_M.gguf");
        assert_eq!(details[0].size_bytes, 16384);
        assert_eq!(
            details[0].sha256.as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
        assert_eq!(details[0].quantization.as_deref(), Some("Q5_K_M"));
    }

    #[test]
    fn runnable_filename_rejects_projector_gguf() {
        assert!(validate_runnable_gguf_filename("model-Q4_K_M.gguf").is_ok());
        assert!(validate_runnable_gguf_filename("mmproj-model-f16.gguf").is_err());
        assert!(validate_runnable_gguf_filename("vision-projector.gguf").is_err());
    }

    #[test]
    fn downloaded_model_metadata_preserves_selected_file_identity() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-hf-metadata-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("temp model dir should exist");
        let file = "Tiny-UD-Q4_K_M.gguf";
        fs::write(dir.join(file), b"tiny model").expect("gguf should write");
        let mut detail = gguf_file_detail(&dir, file, None).expect("detail should load");
        detail.sha256 = Some("abc123".into());
        write_metadata(
            "org/model-GGUF",
            &dir,
            Some("refs/pr/7"),
            Some(file),
            Some(&detail),
        )
        .expect("metadata should write");

        let dto = model_dto("org/model-GGUF", &dir).expect("dto should load");
        assert_eq!(dto.revision.as_deref(), Some("refs/pr/7"));
        assert_eq!(dto.selected_file.as_deref(), Some(file));
        assert_eq!(dto.gguf_file_details.len(), 1);
        assert_eq!(dto.gguf_file_details[0].file, file);
        assert_eq!(dto.gguf_file_details[0].sha256.as_deref(), Some("abc123"));
        assert_eq!(
            dto.gguf_file_details[0].quantization.as_deref(),
            Some("Q4_K_M")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn filename_validation_rejects_unsafe_or_non_gguf_paths() {
        assert_eq!(
            normalized_optional_filename(Some("nested/model.Q4_K_M.gguf")).unwrap(),
            Some("nested/model.Q4_K_M.gguf".into())
        );
        assert!(normalized_optional_filename(Some("../model.gguf")).is_err());
        assert!(normalized_optional_filename(Some("/tmp/model.gguf")).is_err());
        assert!(normalized_optional_filename(Some("model.safetensors")).is_err());
    }

    #[test]
    fn running_model_path_detects_model_directory() {
        let dir = PathBuf::from("/tmp/benchforge/models/org__model");
        assert!(model_path_is_inside_dir(
            "/tmp/benchforge/models/org__model/model.gguf",
            &dir
        ));
        assert!(!model_path_is_inside_dir(
            "/tmp/benchforge/models/other/model.gguf",
            &dir
        ));
    }

    #[test]
    fn memory_estimate_scales_with_model_and_context() {
        let small = estimate_runtime_memory_bytes(1024 * 1024 * 1024, 2048);
        let large = estimate_runtime_memory_bytes(2 * 1024 * 1024 * 1024, 4096);
        assert!(large > small);
        assert_eq!(format_bytes_u64(1024 * 1024 * 1024), "1.0 GB");
        assert!(has_quantization_hint("model-Q4_K_M.gguf"));
        assert!(!has_quantization_hint("model.gguf"));
    }

    #[test]
    fn local_model_preflight_findings_error_when_memory_exceeds_system_memory() {
        let model_size = 8 * 1024 * 1024 * 1024;
        let system_memory = Some(4 * 1024 * 1024 * 1024);
        let findings =
            local_model_preflight_findings("Model-Q4_K_M.gguf", model_size, 2048, system_memory);
        assert!(findings
            .errors
            .iter()
            .any(|error| error.contains("exceeds system memory")));
    }

    #[test]
    fn local_model_preflight_findings_keep_warnings_non_fatal() {
        let model_size = 3 * 1024 * 1024 * 1024;
        let system_memory = Some(32 * 1024 * 1024 * 1024);
        let findings =
            local_model_preflight_findings("model.gguf", model_size, 16_384, system_memory);
        assert!(findings.errors.is_empty());
        assert!(findings
            .warnings
            .iter()
            .any(|warning| warning.contains("Context 16384")));
        assert!(findings
            .warnings
            .iter()
            .any(|warning| warning.contains("quantization marker")));
    }

    #[test]
    fn local_server_settings_reject_bad_ports_and_contexts() {
        assert!(validate_local_server_settings(8080, 2048).is_ok());
        assert!(validate_local_server_settings(80, 2048).is_err());
        assert!(validate_local_server_settings(8080, 64).is_err());
        assert!(validate_local_server_settings(8080, 200_000).is_err());
    }

    #[test]
    fn first_openai_model_id_reads_common_model_list_shapes() {
        assert_eq!(
            first_openai_model_id(r#"{"data":[{"id":"tinygemma3-Q8_0.gguf"}]}"#),
            Some("tinygemma3-Q8_0.gguf".into())
        );
        assert_eq!(
            first_openai_model_id(r#"{"models":[{"name":"llama-local"},{"model":"backup"}]}"#),
            Some("llama-local".into())
        );
        assert_eq!(
            first_openai_model_id(r#"{"data":[{"id":"   "},{"model":"served-model"}]}"#),
            Some("served-model".into())
        );
        assert_eq!(first_openai_model_id(r#"{"data":[]}"#), None);
    }

    #[test]
    fn startup_error_includes_log_tail() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-llama-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("temp dir should exist");
        let log_path = dir.join("llama.log");
        fs::write(&log_path, "first line\nsecond line\nfatal bind error")
            .expect("log should write");
        let error = llama_startup_error("llama-server failed.", &log_path);
        assert!(error.contains("llama-server failed."));
        assert!(error.contains("fatal bind error"));
        assert!(error.contains("Log path:"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn status_clears_exited_local_server_process() {
        let state = store::AppState {
            conn: std::sync::Mutex::new(store::open_memory().expect("store should open")),
            hf_server: std::sync::Mutex::new(Some(store::exited_hf_server_for_tests())),
        };

        let status = super::status(&state).expect("status should load");

        assert!(!status.server_running);
        assert!(status
            .detail
            .contains("previous llama-server exited with status"));
        assert!(state.hf_server.lock().unwrap().is_none());
    }

    #[test]
    fn status_reports_model_cache_summary() {
        let repo_id = format!("test/status-cache-{}", uuid::Uuid::new_v4());
        let dir = model_dir(&repo_id);
        fs::create_dir_all(&dir).expect("model dir should be created");
        fs::write(dir.join("model.gguf"), b"test-model-bytes").expect("model file should write");
        let state = store::AppState {
            conn: std::sync::Mutex::new(store::open_memory().expect("store should open")),
            hf_server: std::sync::Mutex::new(None),
        };

        let status = super::status(&state).expect("status should load");

        assert!(status.cache_dir.contains(".benchforge"));
        assert!(status.cache_dir.contains("models"));
        assert!(status.cache_size_bytes >= "test-model-bytes".len() as u64);
        assert!(status.models.iter().any(|model| model.repo_id == repo_id));
        if status.python_available {
            assert!(status.python_version.is_some());
        } else {
            assert!(status.python_version.is_none());
            assert!(!status.python_supported);
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn status_deserializes_old_snapshots_without_python_fields() {
        let status: HuggingFaceStatusDto = serde_json::from_value(serde_json::json!({
            "tokenAvailable": true,
            "hfCliAvailable": true,
            "llamaServerAvailable": true,
            "serverRunning": true,
            "serverModelId": "served-model",
            "detail": "ready",
            "models": []
        }))
        .expect("old status snapshots should remain readable");

        assert!(status.token_available);
        assert!(!status.python_available);
        assert!(!status.python_supported);
        assert_eq!(status.python_version, None);
        assert!(status.hf_cli_available);
    }

    #[test]
    fn preflight_errors_for_projector_file() {
        let repo_id = format!("test/preflight-{}", uuid::Uuid::new_v4());
        let dir = model_dir(&repo_id);
        fs::create_dir_all(&dir).expect("model dir should be created");
        fs::write(dir.join("mmproj-test.gguf"), b"tiny").expect("tiny gguf should be written");
        let result = preflight_model(StartModelRequest {
            repo_id: repo_id.clone(),
            filename: Some("mmproj-test.gguf".into()),
            port: 8080,
            context: 256,
            register_target_after_start: false,
            run_connectivity_after_start: false,
            auto_benchmark_pack_id: None,
            auto_compare_after_start: false,
            auto_benchmark_target_ids: vec![],
        })
        .expect("preflight should run");
        assert_eq!(result.status, "error");
        assert!(result
            .errors
            .iter()
            .any(|warning| warning.contains("projector")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dry_run_summary_ignores_warning_prefix() {
        let stdout = b"Warning: rate limit\n[{\"file\":\"model.gguf\",\"size\":\"1.0M\"}]";
        let plan = parse_dry_run_plan(stdout).expect("dry run should parse");
        assert_eq!(plan.summary, "Planned download: model.gguf (1.0M)");
        assert_eq!(plan.planned_bytes, Some(1_000_000));
    }

    #[test]
    fn direct_download_plan_uses_file_metadata_and_mentions_fallback() {
        let details = vec![GgufFileDto {
            file: "nested/model-q4.gguf".into(),
            size_bytes: 42 * 1024 * 1024,
            sha256: None,
            quantization: Some("Q4".into()),
        }];
        let plan = direct_download_plan_from_details(
            "nested/model-q4.gguf",
            &details,
            Some("\nPython 3.10+ is required\nmore detail"),
        );

        assert_eq!(plan.planned_bytes, Some(42 * 1024 * 1024));
        assert!(plan.summary.contains("Planned direct download"));
        assert!(plan.summary.contains("42 MB"));
        assert!(plan.summary.contains("Python 3.10+ is required"));
    }

    #[test]
    fn hf_resolve_url_encodes_repo_revision_and_nested_file() {
        let url = hf_resolve_url(
            "org name/model GGUF",
            Some("refs/pr/1"),
            "sub dir/model q4.gguf",
        );

        assert_eq!(
            url,
            "https://huggingface.co/org%20name/model%20GGUF/resolve/refs/pr/1/sub%20dir/model%20q4.gguf"
        );
    }

    #[test]
    fn hf_size_parser_handles_common_units() {
        assert_eq!(parse_hf_size_bytes("807.0"), Some(807));
        assert_eq!(parse_hf_size_bytes("1.5K"), Some(1500));
        assert_eq!(parse_hf_size_bytes("2.0 MB"), Some(2_000_000));
        assert_eq!(parse_hf_size_bytes("47.2M"), Some(47_200_000));
        assert_eq!(parse_hf_size_bytes("2.0 MiB"), Some(2 * 1024 * 1024));
        assert_eq!(parse_hf_size_bytes("3.0GiB"), Some(3 * 1024 * 1024 * 1024));
        assert_eq!(parse_hf_size_bytes("unknown size"), None);
    }

    #[test]
    fn existing_download_match_requires_known_complete_size() {
        assert!(existing_download_matches_plan(Some(1024), Some(1024)));
        assert!(existing_download_matches_plan(Some(2048), Some(1024)));
        assert!(!existing_download_matches_plan(Some(512), Some(1024)));
        assert!(!existing_download_matches_plan(Some(1024), None));
        assert!(!existing_download_matches_plan(Some(0), Some(1024)));
        assert!(!existing_download_matches_plan(None, Some(1024)));
    }

    #[test]
    fn existing_download_reuse_accepts_saved_sha_match() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-hf-reuse-ok-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("model dir should be created");
        let file = "model.gguf";
        let path = dir.join(file);
        fs::write(&path, b"model-bytes").expect("model file should write");
        let sha256 = checksum_file(&path).expect("checksum should compute");
        let detail = GgufFileDto {
            file: file.into(),
            size_bytes: 11,
            sha256: Some(sha256),
            quantization: None,
        };
        write_metadata("org/model", &dir, None, Some(file), Some(&detail))
            .expect("metadata should write");

        let validation =
            validate_existing_download_reuse(&dir, file, &path, Some(11), Some(11), None);

        assert!(validation.reusable);
        assert!(!validation.remove_before_download);
        assert!(validation.log.unwrap_or_default().contains("saved SHA-256"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn existing_download_reuse_rejects_saved_sha_mismatch() {
        let dir = std::env::temp_dir().join(format!(
            "benchforge-hf-reuse-mismatch-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("model dir should be created");
        let file = "model.gguf";
        let path = dir.join(file);
        fs::write(&path, b"model-bytes").expect("model file should write");
        let detail = GgufFileDto {
            file: file.into(),
            size_bytes: 11,
            sha256: Some("0000000000000000000000000000000000000000000000000000000000000000".into()),
            quantization: None,
        };
        write_metadata("org/model", &dir, None, Some(file), Some(&detail))
            .expect("metadata should write");

        let validation =
            validate_existing_download_reuse(&dir, file, &path, Some(11), Some(11), None);

        assert!(!validation.reusable);
        assert!(validation.remove_before_download);
        assert!(validation
            .log
            .unwrap_or_default()
            .contains("failed saved SHA-256"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn existing_download_reuse_rejects_huggingface_sha_mismatch_before_saved_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "benchforge-hf-reuse-remote-mismatch-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("model dir should be created");
        let file = "model.gguf";
        let path = dir.join(file);
        fs::write(&path, b"model-bytes").expect("model file should write");

        let validation = validate_existing_download_reuse(
            &dir,
            file,
            &path,
            Some(11),
            Some(11),
            Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"),
        );

        assert!(!validation.reusable);
        assert!(validation.remove_before_download);
        assert!(validation
            .log
            .unwrap_or_default()
            .contains("failed Hugging Face SHA-256"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn finalize_downloaded_model_rejects_huggingface_sha_mismatch() {
        let dir = std::env::temp_dir().join(format!(
            "benchforge-hf-finalize-mismatch-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("model dir should be created");
        let file = "model.gguf";
        let expected_file = dir.join(file);
        fs::write(&expected_file, b"model-bytes").expect("model file should write");
        let prepared = PreparedDownload {
            selected_file: file.into(),
            revision: Some("refs/pr/7".into()),
            model_dir: dir.clone(),
            hf_home_dir: download_hf_home_dir(&dir),
            expected_file,
            expected_sha256: Some(
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".into(),
            ),
            plan: DownloadPlan {
                summary: "planned".into(),
                planned_bytes: Some(11),
            },
            disk_space_log: "disk ok".into(),
            existing_bytes: Some(11),
            partial_bytes: 0,
            already_downloaded: false,
            existing_integrity_log: None,
            remove_existing_before_download: false,
        };

        let error = finalize_downloaded_model("org/model", &prepared, "downloaded".into())
            .expect_err("mismatched remote hash should fail finalization");

        assert!(error.contains("failed Hugging Face SHA-256 validation"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_download_bytes_counts_incomplete_fragments() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-hf-partials-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join(".cache/huggingface/download"))
            .expect("partial cache dir should be created");
        fs::write(dir.join("model.gguf.incomplete"), b"abc").expect("partial file should write");
        fs::write(dir.join(".cache/huggingface/download/blob.part"), b"abcdef")
            .expect("nested partial should write");
        fs::write(dir.join("complete.gguf"), b"abcdefghij").expect("complete file should write");

        assert_eq!(partial_download_bytes(&dir), 9);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn model_scoped_hf_home_keeps_cache_inside_inventory_excluded_dir() {
        let dir = PathBuf::from("/tmp/benchforge/models/org__model");
        assert_eq!(
            download_hf_home_dir(&dir),
            PathBuf::from("/tmp/benchforge/models/org__model/.cache/hf-home")
        );
    }

    #[test]
    fn download_progress_reports_percent_and_identity() {
        let dir =
            std::env::temp_dir().join(format!("benchforge-hf-progress-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("temp dir should exist");
        let expected_file = dir.join("model-q4.gguf");
        fs::write(&expected_file, vec![0_u8; 50]).expect("partial file should write");
        let request = DownloadModelRequest {
            repo_id: "org/model-GGUF".into(),
            filename: Some("model-q4.gguf".into()),
            revision: None,
            download_id: Some("download-1".into()),
            start_after_download: false,
            run_connectivity_after_start: false,
            auto_benchmark_pack_id: None,
            auto_compare_after_start: false,
            auto_benchmark_target_ids: vec![],
            start_port: None,
            start_context: None,
        };
        let prepared = PreparedDownload {
            selected_file: "model-q4.gguf".into(),
            revision: None,
            model_dir: dir.clone(),
            hf_home_dir: download_hf_home_dir(&dir),
            expected_file,
            expected_sha256: None,
            plan: DownloadPlan {
                summary: "planned".into(),
                planned_bytes: Some(100),
            },
            disk_space_log: "disk ok".into(),
            existing_bytes: None,
            partial_bytes: 0,
            already_downloaded: false,
            existing_integrity_log: None,
            remove_existing_before_download: false,
        };
        let mut events = Vec::new();
        let transferred = current_download_bytes(&prepared);
        emit_download_progress(
            &request,
            &prepared,
            &mut |progress| events.push(progress),
            "running",
            transferred,
            "downloading",
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].download_id.as_deref(), Some("download-1"));
        assert_eq!(events[0].repo_id, "org/model-GGUF");
        assert_eq!(events[0].transferred_bytes, 50);
        assert_eq!(events[0].percent, Some(50.0));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn download_job_from_record_reports_percent_and_model() {
        let record = store::HfDownloadJobRecord {
            id: "job-1".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("model-Q4_K_M.gguf".into()),
            status: "completed".into(),
            message: "Downloaded".into(),
            started_at: "2026-07-06T12:00:00Z".into(),
            finished_at: Some("2026-07-06T12:01:00Z".into()),
            planned_bytes: Some(100),
            transferred_bytes: 75,
            local_dir: Some("/tmp/model".into()),
            error: None,
            request: serde_json::json!({
                "repoId": "org/model-GGUF",
                "startAfterDownload": true,
                "runConnectivityAfterStart": true,
                "autoBenchmarkPackId": "llm-basics",
                "autoBenchmarkTargetIds": ["hf-local-target", "cloud-priced"],
                "startPort": 8081,
                "startContext": 4096
            }),
            model: Some(serde_json::json!({
                "repoId": "org/model-GGUF",
                "path": "/tmp/model",
                "files": ["model-Q4_K_M.gguf"],
                "ggufFiles": ["model-Q4_K_M.gguf"],
                "ggufFileDetails": [{"file": "model-Q4_K_M.gguf", "sizeBytes": 100, "sha256": null, "quantization": "Q4_K_M"}],
                "sizeBytes": 100,
                "selectedFile": "model-Q4_K_M.gguf",
                "downloadLog": "ok"
            })),
        };

        let dto = download_job_from_record(record).expect("download job dto should parse");

        assert_eq!(dto.percent, Some(100.0));
        assert!(dto.start_after_download);
        assert!(dto.run_connectivity_after_start);
        assert_eq!(dto.auto_benchmark_pack_id.as_deref(), Some("llm-basics"));
        assert_eq!(dto.start_port, Some(8081));
        assert_eq!(dto.start_context, Some(4096));
        assert_eq!(
            dto.model.unwrap().selected_file.as_deref(),
            Some("model-Q4_K_M.gguf")
        );
    }

    #[test]
    fn download_job_rejects_invalid_auto_start_settings() {
        let conn = store::open_memory().expect("store should open");
        let error = start_download_job(
            &conn,
            DownloadModelRequest {
                repo_id: "org/model-GGUF".into(),
                filename: Some("model-Q4_K_M.gguf".into()),
                revision: None,
                download_id: None,
                start_after_download: true,
                run_connectivity_after_start: true,
                auto_benchmark_pack_id: Some("llm-basics".into()),
                auto_compare_after_start: false,
                auto_benchmark_target_ids: vec![],
                start_port: Some(80),
                start_context: Some(2048),
            },
        )
        .expect_err("invalid auto-start port should reject the job");

        assert!(error.contains("port must be between"));
    }

    #[test]
    fn download_retry_request_reuses_persisted_handoff_settings() {
        let record = store::HfDownloadJobRecord {
            id: "failed-download".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("fallback-Q4_K_M.gguf".into()),
            status: "failed".into(),
            message: "failed".into(),
            started_at: "2026-07-06T12:00:00Z".into(),
            finished_at: Some("2026-07-06T12:01:00Z".into()),
            planned_bytes: None,
            transferred_bytes: 0,
            local_dir: None,
            error: Some("network".into()),
            request: serde_json::json!({
                "repoId": "org/model-GGUF",
                "filename": "model-Q4_K_M.gguf",
                "revision": "main",
                "downloadId": "old-progress-id",
                "startAfterDownload": true,
                "runConnectivityAfterStart": true,
                "autoBenchmarkPackId": "llm-basics",
                "autoBenchmarkTargetIds": ["hf-local-target", "cloud-priced"],
                "startPort": 8081,
                "startContext": 4096
            }),
            model: None,
        };

        let request = download_request_from_job(&record);

        assert_eq!(request.repo_id, "org/model-GGUF");
        assert_eq!(request.filename.as_deref(), Some("model-Q4_K_M.gguf"));
        assert_eq!(request.revision.as_deref(), Some("main"));
        assert_eq!(request.download_id, None);
        assert!(request.start_after_download);
        assert!(request.run_connectivity_after_start);
        assert_eq!(
            request.auto_benchmark_pack_id.as_deref(),
            Some("llm-basics")
        );
        assert_eq!(
            request.auto_benchmark_target_ids,
            vec!["hf-local-target".to_string(), "cloud-priced".to_string()]
        );
        assert_eq!(request.start_port, Some(8081));
        assert_eq!(request.start_context, Some(4096));
    }

    #[test]
    fn download_retry_rejects_completed_jobs() {
        let conn = store::open_memory().expect("store should open");
        let record = store::HfDownloadJobRecord {
            id: "completed-download".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("model-Q4_K_M.gguf".into()),
            status: "completed".into(),
            message: "Downloaded".into(),
            started_at: "2026-07-06T12:00:00Z".into(),
            finished_at: Some("2026-07-06T12:01:00Z".into()),
            planned_bytes: Some(100),
            transferred_bytes: 100,
            local_dir: None,
            error: None,
            request: serde_json::json!({"repoId": "org/model-GGUF", "filename": "model-Q4_K_M.gguf"}),
            model: None,
        };
        store::insert_hf_download_job(&conn, &record).expect("job should insert");

        let error = retry_download_job(&conn, "completed-download")
            .expect_err("completed downloads should not be retried");

        assert!(error.contains("only failed or cancelled"));
    }

    #[test]
    fn download_retry_rejects_active_jobs() {
        let conn = store::open_memory().expect("store should open");
        for status in ["queued", "running", "cancelling"] {
            let id = format!("active-download-{status}");
            store::insert_hf_download_job(
                &conn,
                &store::HfDownloadJobRecord {
                    id: id.clone(),
                    repo_id: "org/model-GGUF".into(),
                    selected_file: Some("model-Q4_K_M.gguf".into()),
                    status: status.into(),
                    message: "active".into(),
                    started_at: "2026-07-06T12:00:00Z".into(),
                    finished_at: None,
                    planned_bytes: Some(100),
                    transferred_bytes: 25,
                    local_dir: None,
                    error: None,
                    request: serde_json::json!({
                        "repoId": "org/model-GGUF",
                        "filename": "model-Q4_K_M.gguf"
                    }),
                    model: None,
                },
            )
            .expect("job should insert");

            let error =
                retry_download_job(&conn, &id).expect_err("active downloads should not retry");

            assert!(error.contains("active_hf_download_retry_blocked"));
        }
    }

    #[test]
    fn model_delete_blocks_active_lifecycle_jobs() {
        let conn = store::open_memory().expect("store should open");
        store::insert_hf_download_job(
            &conn,
            &store::HfDownloadJobRecord {
                id: "download-active-123".into(),
                repo_id: "org/model-GGUF".into(),
                selected_file: Some("model-Q4_K_M.gguf".into()),
                status: "running".into(),
                message: "downloading".into(),
                started_at: "2026-07-06T12:00:00Z".into(),
                finished_at: None,
                planned_bytes: Some(100),
                transferred_bytes: 25,
                local_dir: Some("/tmp/model".into()),
                error: None,
                request: serde_json::json!({"repoId": "org/model-GGUF"}),
                model: None,
            },
        )
        .expect("download job should insert");

        let blocker = active_model_lifecycle_job_blocker(&conn, "org/model-GGUF")
            .expect("blocker check should work")
            .expect("active download should block deletion");

        assert!(blocker.contains("download job"));
        assert!(blocker.contains("running"));
        assert!(blocker.contains("download"));
    }

    #[test]
    fn model_delete_blocks_active_server_start_but_ignores_finished_jobs() {
        let conn = store::open_memory().expect("store should open");
        store::insert_hf_download_job(
            &conn,
            &store::HfDownloadJobRecord {
                id: "download-done".into(),
                repo_id: "org/model-GGUF".into(),
                selected_file: Some("model-Q4_K_M.gguf".into()),
                status: "completed".into(),
                message: "downloaded".into(),
                started_at: "2026-07-06T12:00:00Z".into(),
                finished_at: Some("2026-07-06T12:01:00Z".into()),
                planned_bytes: Some(100),
                transferred_bytes: 100,
                local_dir: Some("/tmp/model".into()),
                error: None,
                request: serde_json::json!({"repoId": "org/model-GGUF"}),
                model: None,
            },
        )
        .expect("download job should insert");
        store::insert_hf_server_job(
            &conn,
            &store::HfServerJobRecord {
                id: "server-active-123".into(),
                repo_id: "org/model-GGUF".into(),
                selected_file: Some("model-Q4_K_M.gguf".into()),
                port: 8080,
                context: 2048,
                status: "queued".into(),
                message: "queued".into(),
                started_at: "2026-07-06T12:02:00Z".into(),
                finished_at: None,
                error: None,
                request: serde_json::json!({"repoId": "org/model-GGUF"}),
                server_status: None,
            },
        )
        .expect("server job should insert");

        let blocker = active_model_lifecycle_job_blocker(&conn, "org/model-GGUF")
            .expect("blocker check should work")
            .expect("active server start should block deletion");

        assert!(blocker.contains("server start job"));
        assert!(blocker.contains("queued"));
        assert!(blocker.contains("server-a"));
        assert!(active_model_lifecycle_job_blocker(&conn, "org/other-GGUF")
            .expect("blocker check should work")
            .is_none());
    }

    #[test]
    fn server_job_from_record_preserves_status() {
        let record = store::HfServerJobRecord {
            id: "server-job-1".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("model-Q4_K_M.gguf".into()),
            port: 8080,
            context: 2048,
            status: "completed".into(),
            message: "ready".into(),
            started_at: "2026-07-06T12:00:00Z".into(),
            finished_at: Some("2026-07-06T12:01:00Z".into()),
            error: None,
            request: serde_json::json!({
                "repoId": "org/model-GGUF",
                "filename": "model-Q4_K_M.gguf",
                "port": 8080,
                "context": 2048,
                "registerTargetAfterStart": true,
                "runConnectivityAfterStart": true,
                "autoBenchmarkPackId": "llm-basics",
                "autoBenchmarkTargetIds": ["hf-local-target", "cloud-priced"]
            }),
            server_status: Some(serde_json::json!({
                "tokenAvailable": true,
                "hfCliAvailable": true,
                "llamaServerAvailable": true,
                "serverRunning": true,
                "serverModelId": "served-model",
                "detail": "ready",
                "models": []
            })),
        };

        let dto = server_job_from_record(record).expect("server job dto should parse");

        assert_eq!(dto.status, "completed");
        assert_eq!(dto.selected_file.as_deref(), Some("model-Q4_K_M.gguf"));
        assert!(dto.register_target_after_start);
        assert!(dto.run_connectivity_after_start);
        assert_eq!(dto.auto_benchmark_pack_id.as_deref(), Some("llm-basics"));
        assert_eq!(
            dto.server_status
                .expect("server status should parse")
                .server_model_id
                .as_deref(),
            Some("served-model")
        );
    }

    #[test]
    fn server_retry_request_reuses_persisted_handoff_settings() {
        let record = store::HfServerJobRecord {
            id: "failed-server".into(),
            repo_id: "org/model-GGUF".into(),
            selected_file: Some("model-Q4_K_M.gguf".into()),
            port: 8081,
            context: 4096,
            status: "failed".into(),
            message: "failed".into(),
            started_at: "2026-07-06T12:00:00Z".into(),
            finished_at: Some("2026-07-06T12:01:00Z".into()),
            error: Some("port busy".into()),
            request: serde_json::json!({
                "repoId": "org/model-GGUF",
                "filename": "model-Q4_K_M.gguf",
                "port": 8081,
                "context": 4096,
                "registerTargetAfterStart": true,
                "runConnectivityAfterStart": true,
                "autoBenchmarkPackId": "llm-basics",
                "autoBenchmarkTargetIds": ["hf-local-target", "cloud-priced"]
            }),
            server_status: None,
        };

        let request = server_request_from_job(&record);

        assert_eq!(request.repo_id, "org/model-GGUF");
        assert_eq!(request.filename.as_deref(), Some("model-Q4_K_M.gguf"));
        assert_eq!(request.port, 8081);
        assert_eq!(request.context, 4096);
        assert!(request.register_target_after_start);
        assert!(request.run_connectivity_after_start);
        assert_eq!(
            request.auto_benchmark_pack_id.as_deref(),
            Some("llm-basics")
        );
        assert_eq!(
            request.auto_benchmark_target_ids,
            vec!["hf-local-target".to_string(), "cloud-priced".to_string()]
        );

        let legacy_record = store::HfServerJobRecord {
            request: serde_json::json!({
                "repoId": "org/model-GGUF",
                "filename": "model-Q4_K_M.gguf",
                "registerTargetAfterStart": true
            }),
            port: 9090,
            context: 8192,
            ..record
        };
        let legacy_request = server_request_from_job(&legacy_record);
        assert_eq!(legacy_request.port, 9090);
        assert_eq!(legacy_request.context, 8192);
    }

    #[test]
    fn server_retry_rejects_active_jobs() {
        let conn = store::open_memory().expect("store should open");
        for status in ["queued", "running", "cancelling"] {
            let id = format!("active-server-{status}");
            store::insert_hf_server_job(
                &conn,
                &store::HfServerJobRecord {
                    id: id.clone(),
                    repo_id: "org/model-GGUF".into(),
                    selected_file: Some("model-Q4_K_M.gguf".into()),
                    port: 8080,
                    context: 2048,
                    status: status.into(),
                    message: "active".into(),
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
            .expect("job should insert");

            let error =
                retry_server_job(&conn, &id).expect_err("active server starts should not retry");

            assert!(error.contains("active_hf_server_retry_blocked"));
        }
    }

    #[test]
    fn friendly_download_error_adds_actionable_hints() {
        let error = friendly_download_error("Error: Repository not found.");
        assert!(error.contains("Hint:"));
        assert!(error.contains("model license"));
        let missing_file = friendly_download_error("404 Client Error: Entry not found");
        assert!(missing_file.contains("exact .gguf filename"));
    }
}
