use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{runner, store, targeting};

const PROMPT_DEFAULT_MAX_TOKENS: u64 = 512;
const WORKSPACE_DEFAULT_MAX_TOKENS: u64 = 4096;

#[derive(Clone, Copy)]
struct GenerationSnapshotDefaults {
    default_max_tokens: u64,
    has_prompt_tasks: bool,
    has_workspace_tasks: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RetryScope {
    target_ids: Vec<String>,
    task_ids: Vec<String>,
    repetitions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunJobReplayDto {
    pub mode: String,
    pub source_job_id: String,
    pub source_run_group_id: String,
    pub source_target_count: usize,
    pub source_task_count: usize,
    pub source_repetitions: u32,
    pub target_count: usize,
    pub task_count: usize,
    pub repetitions: u32,
    pub scoped: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunJobDto {
    pub id: String,
    #[serde(rename = "runGroupId")]
    pub run_group_id: String,
    #[serde(rename = "benchmarkPackId")]
    pub benchmark_pack_id: String,
    pub status: String,
    pub message: String,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    #[serde(rename = "finishedAt")]
    pub finished_at: Option<String>,
    pub total: usize,
    pub completed: usize,
    pub results: Vec<store::ResultRecord>,
    pub error: Option<String>,
    pub settings: RunJobSettingsDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunJobSettingsDto {
    #[serde(rename = "targetCount")]
    pub target_count: usize,
    #[serde(rename = "taskCount")]
    pub task_count: usize,
    pub repetitions: u32,
    #[serde(rename = "warmupRuns")]
    pub warmup_runs: u32,
    pub concurrency: u32,
    pub docker: bool,
    #[serde(rename = "maxCostUsd")]
    pub max_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay: Option<RunJobReplayDto>,
}

pub fn start_quick_smoke_job(
    conn: &Connection,
    request: runner::RunQuickSmokeRequest,
) -> Result<RunJobDto, String> {
    start_quick_smoke_job_with_replay(conn, request, None)
}

fn start_quick_smoke_job_with_replay(
    conn: &Connection,
    mut request: runner::RunQuickSmokeRequest,
    replay: Option<RunJobReplayDto>,
) -> Result<RunJobDto, String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let run_group_id = uuid::Uuid::new_v4().to_string();
    let started_at = store::now();
    let target_ids = if request.target_ids.is_empty() {
        vec!["mock-agent".to_string()]
    } else {
        request.target_ids.clone()
    };

    request.target_ids = target_ids.clone();
    request.repetitions = request.repetitions.max(1);
    request.warmup_runs = request.warmup_runs.min(20);
    request.concurrency = runner::normalized_concurrency(request.concurrency);
    request.run_group_id = Some(run_group_id.clone());

    let pack = runner::load_pack(&request.benchmark_pack_id)?;
    let tasks = runner::select_tasks_for_run(runner::load_tasks(&pack)?, &request.task_ids)?;
    let available_targets = store::list_targets(conn).map_err(|err| err.to_string())?;
    runner::validate_docker_scoring_preflight_for_tasks(&tasks, request.docker, &|| false)?;
    runner::validate_target_compatibility(&pack, &tasks, &available_targets, &request.target_ids)?;
    runner::validate_target_runtime_preflight_for_tasks(
        &available_targets,
        &request.target_ids,
        &tasks,
    )?;

    let selected_targets = run_group_targets(conn, &target_ids)?;
    enforce_run_cost_limit(&request, &selected_targets)?;
    let run_group_config =
        run_group_config_snapshot(&request, &selected_targets, &tasks, replay.as_ref());

    store::insert_run_group(
        conn,
        &run_group_id,
        &request.benchmark_pack_id,
        &target_ids,
        "queued",
        &started_at,
        &run_group_config,
    )
    .map_err(|err| err.to_string())?;

    let mut request_json = serde_json::to_value(&request).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(replay) = &replay {
        if let Value::Object(map) = &mut request_json {
            map.insert(
                "replay".into(),
                serde_json::to_value(replay).unwrap_or_else(|_| serde_json::json!({})),
            );
        }
    }

    let record = store::RunJobRecord {
        id: job_id.clone(),
        run_group_id: run_group_id.clone(),
        benchmark_pack_id: request.benchmark_pack_id.clone(),
        status: "queued".into(),
        message: format!("Queued {} run", request.benchmark_pack_id),
        started_at,
        finished_at: None,
        total: 0,
        completed: 0,
        error: None,
        request: request_json,
        result_run_ids: vec![],
    };
    store::insert_run_job(conn, &record).map_err(|err| err.to_string())?;

    let worker_job_id = job_id.clone();
    let worker_group_id = run_group_id.clone();
    std::thread::spawn(move || {
        let Ok(conn) = store::open_app() else {
            return;
        };

        if cancellation_requested(&conn, &worker_job_id) {
            finish_cancelled(&conn, &worker_job_id, &worker_group_id);
            return;
        }

        let _ = store::update_run_group_status(&conn, &worker_group_id, "running", None);
        let _ = store::update_run_job_progress(
            &conn,
            &worker_job_id,
            "running",
            "Opening BenchForge store",
            0,
            0,
        );

        let cancel_job_id = worker_job_id.clone();
        let cancellation_check: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            store::open_app()
                .ok()
                .map(|conn| cancellation_requested(&conn, &cancel_job_id))
                .unwrap_or(false)
        });

        let result = runner::run_quick_smoke_with_shared_cancel(
            &conn,
            request,
            |progress| {
                if !cancellation_requested(&conn, &worker_job_id) {
                    let _ = store::update_run_job_progress(
                        &conn,
                        &worker_job_id,
                        "running",
                        &progress.message,
                        progress.total,
                        progress.completed,
                    );
                }
            },
            cancellation_check,
        );

        let finished_at = store::now();
        match result {
            Ok(results) => {
                let result_ids = results
                    .iter()
                    .map(|result| result.id.clone())
                    .collect::<Vec<_>>();
                let total = current_total(&conn, &worker_job_id).max(result_ids.len());
                let message = format!("Completed {} benchmark task runs", result_ids.len());
                let _ = store::finish_run_job(
                    &conn,
                    &worker_job_id,
                    "completed",
                    &message,
                    total,
                    result_ids.len(),
                    &finished_at,
                    None,
                    &result_ids,
                );
                let _ = store::update_run_group_status(
                    &conn,
                    &worker_group_id,
                    "completed",
                    Some(&finished_at),
                );
            }
            Err(err) if err == "cancelled" => {
                finish_cancelled(&conn, &worker_job_id, &worker_group_id);
            }
            Err(err) => {
                finish_failed_with_partial_results(
                    &conn,
                    &worker_job_id,
                    &worker_group_id,
                    &err,
                    &finished_at,
                );
            }
        }
    });

    get_job(conn, &job_id)?.ok_or_else(|| "run job was not persisted".into())
}

pub fn cancel_job(conn: &Connection, id: &str) -> Result<Option<RunJobDto>, String> {
    let Some(job) = store::get_run_job(conn, id).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if matches!(job.status.as_str(), "queued" | "running") {
        store::request_cancel_run_job(conn, id).map_err(|err| err.to_string())?;
    }
    get_job(conn, id)
}

pub fn duplicate_job(conn: &Connection, id: &str) -> Result<Option<RunJobDto>, String> {
    let Some(job) = store::get_run_job(conn, id).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if run_job_is_active(&job) {
        return Err("active_job_replay_blocked: wait for the run job to finish or cancel it before duplicating".into());
    }
    let original_request = request_from_job(&job)?;
    let request = original_request.clone();
    let replay = replay_metadata("duplicate", &job, &original_request, &request);
    start_quick_smoke_job_with_replay(conn, request, Some(replay)).map(Some)
}

pub fn retry_job(conn: &Connection, id: &str) -> Result<Option<RunJobDto>, String> {
    let Some(job) = store::get_run_job(conn, id).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if run_job_is_active(&job) {
        return Err("active_job_replay_blocked: wait for the run job to finish or cancel it before retrying".into());
    }
    let results =
        store::list_results_for_group(conn, &job.run_group_id).map_err(|err| err.to_string())?;
    let has_non_passed = results.iter().any(|result| result.status != "passed");
    if !matches!(job.status.as_str(), "failed" | "cancelled") && !has_non_passed {
        return Err("only failed, cancelled, or non-passing jobs can be retried".into());
    }
    let original_request = request_from_job(&job)?;
    let request = retry_request_from_request_results(original_request.clone(), &results)?;
    let replay = replay_metadata("retry", &job, &original_request, &request);
    start_quick_smoke_job_with_replay(conn, request, Some(replay)).map(Some)
}

pub fn clear_finished_jobs(conn: &Connection) -> Result<usize, String> {
    store::clear_terminal_run_jobs(conn).map_err(|err| err.to_string())
}

fn run_job_is_active(job: &store::RunJobRecord) -> bool {
    matches!(job.status.as_str(), "queued" | "running" | "cancelling")
}

pub fn list_jobs(conn: &Connection) -> Result<Vec<RunJobDto>, String> {
    store::list_run_jobs(conn)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|record| job_from_record(conn, record))
        .collect()
}

pub fn get_job(conn: &Connection, id: &str) -> Result<Option<RunJobDto>, String> {
    store::get_run_job(conn, id)
        .map_err(|err| err.to_string())?
        .map(|record| job_from_record(conn, record))
        .transpose()
}

fn job_from_record(conn: &Connection, record: store::RunJobRecord) -> Result<RunJobDto, String> {
    let results =
        store::list_results_for_group(conn, &record.run_group_id).map_err(|err| err.to_string())?;
    Ok(RunJobDto {
        id: record.id,
        run_group_id: record.run_group_id,
        benchmark_pack_id: record.benchmark_pack_id,
        status: record.status,
        message: record.message,
        started_at: record.started_at,
        finished_at: record.finished_at,
        total: record.total,
        completed: record.completed,
        results,
        error: record.error,
        settings: job_settings_from_request(&record.request),
    })
}

fn job_settings_from_request(request: &Value) -> RunJobSettingsDto {
    RunJobSettingsDto {
        target_count: request
            .get("targetIds")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
        task_count: job_task_count_from_request(request),
        repetitions: json_u32(request, "repetitions", 1, 1, 100),
        warmup_runs: json_u32(request, "warmupRuns", 0, 0, 20),
        concurrency: runner::normalized_concurrency(json_u32(request, "concurrency", 1, 1, 8)),
        docker: request
            .get("docker")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        max_cost_usd: request
            .get("maxCostUsd")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0),
        replay: request
            .get("replay")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok()),
    }
}

fn job_task_count_from_request(request: &Value) -> usize {
    let selected_task_count = request
        .get("taskIds")
        .and_then(Value::as_array)
        .map(|task_ids| {
            task_ids
                .iter()
                .filter_map(Value::as_str)
                .filter(|task_id| !task_id.trim().is_empty())
                .collect::<BTreeSet<_>>()
                .len()
        })
        .unwrap_or(0);
    if selected_task_count > 0 {
        return selected_task_count;
    }
    request
        .get("benchmarkPackId")
        .and_then(Value::as_str)
        .and_then(|benchmark_pack_id| {
            runner::load_pack(benchmark_pack_id)
                .and_then(|pack| runner::load_tasks(&pack))
                .ok()
        })
        .map(|tasks| tasks.len())
        .unwrap_or(0)
}

fn json_u32(request: &Value, key: &str, default: u32, min: u32, max: u32) -> u32 {
    request
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn current_total(conn: &Connection, id: &str) -> usize {
    current_counts(conn, id).0
}

fn current_counts(conn: &Connection, id: &str) -> (usize, usize) {
    store::get_run_job(conn, id)
        .ok()
        .flatten()
        .map(|job| (job.total, job.completed))
        .unwrap_or((0, 0))
}

fn request_from_job(job: &store::RunJobRecord) -> Result<runner::RunQuickSmokeRequest, String> {
    let mut request: runner::RunQuickSmokeRequest =
        serde_json::from_value(job.request.clone()).map_err(|err| err.to_string())?;
    request.run_group_id = None;
    request.repetitions = request.repetitions.max(1);
    request.warmup_runs = request.warmup_runs.min(20);
    request.concurrency = runner::normalized_concurrency(request.concurrency);
    Ok(request)
}

fn retry_request_from_job_results(
    job: &store::RunJobRecord,
    results: &[store::ResultRecord],
) -> Result<runner::RunQuickSmokeRequest, String> {
    retry_request_from_request_results(request_from_job(job)?, results)
}

fn retry_request_from_request_results(
    mut request: runner::RunQuickSmokeRequest,
    results: &[store::ResultRecord],
) -> Result<runner::RunQuickSmokeRequest, String> {
    if let Some(scope) = retry_scope_from_results(&request, results)? {
        request.target_ids = scope.target_ids;
        request.task_ids = scope.task_ids;
        request.repetitions = scope.repetitions.max(1);
    }
    Ok(request)
}

fn replay_metadata(
    mode: &str,
    source_job: &store::RunJobRecord,
    source_request: &runner::RunQuickSmokeRequest,
    replay_request: &runner::RunQuickSmokeRequest,
) -> RunJobReplayDto {
    let source_target_count = effective_target_count(source_request);
    let source_task_count = task_count_from_run_request(source_request);
    let source_repetitions = source_request.repetitions.max(1);
    let target_count = effective_target_count(replay_request);
    let task_count = task_count_from_run_request(replay_request);
    let repetitions = replay_request.repetitions.max(1);
    RunJobReplayDto {
        mode: mode.into(),
        source_job_id: source_job.id.clone(),
        source_run_group_id: source_job.run_group_id.clone(),
        source_target_count,
        source_task_count,
        source_repetitions,
        target_count,
        task_count,
        repetitions,
        scoped: target_count < source_target_count
            || task_count < source_task_count
            || repetitions < source_repetitions,
    }
}

fn effective_target_count(request: &runner::RunQuickSmokeRequest) -> usize {
    if request.target_ids.is_empty() {
        1
    } else {
        request.target_ids.len()
    }
}

fn task_count_from_run_request(request: &runner::RunQuickSmokeRequest) -> usize {
    if !request.task_ids.is_empty() {
        return request
            .task_ids
            .iter()
            .filter(|task_id| !task_id.trim().is_empty())
            .collect::<BTreeSet<_>>()
            .len();
    }
    runner::load_pack(&request.benchmark_pack_id)
        .and_then(|pack| runner::load_tasks(&pack))
        .map(|tasks| tasks.len())
        .unwrap_or(0)
}

fn retry_scope_from_results(
    request: &runner::RunQuickSmokeRequest,
    results: &[store::ResultRecord],
) -> Result<Option<RetryScope>, String> {
    let target_ids = if request.target_ids.is_empty() {
        vec!["mock-agent".to_string()]
    } else {
        request.target_ids.clone()
    };
    let pack = runner::load_pack(&request.benchmark_pack_id)?;
    let task_ids = runner::select_tasks_for_run(runner::load_tasks(&pack)?, &request.task_ids)?
        .into_iter()
        .map(|task| task.id)
        .collect::<Vec<_>>();
    if target_ids.is_empty() || task_ids.is_empty() {
        return Ok(None);
    }

    let target_set = target_ids.iter().cloned().collect::<BTreeSet<_>>();
    let task_set = task_ids.iter().cloned().collect::<BTreeSet<_>>();
    let repetitions = request.repetitions.max(1) as usize;
    let mut passed_counts = BTreeMap::<(String, String), usize>::new();
    for result in results {
        if result.benchmark_pack_id != request.benchmark_pack_id
            || !target_set.contains(&result.target_id)
            || !task_set.contains(&result.task_id)
        {
            continue;
        }
        let key = (result.target_id.clone(), result.task_id.clone());
        if result.status == "passed" {
            *passed_counts.entry(key).or_default() += 1;
        }
    }

    let mut needed_pairs = BTreeSet::<(String, String)>::new();
    let mut needed_repetitions = BTreeMap::<(String, String), usize>::new();
    for target_id in &target_ids {
        for task_id in &task_ids {
            let key = (target_id.clone(), task_id.clone());
            let passed = passed_counts.get(&key).copied().unwrap_or(0);
            let missing = repetitions.saturating_sub(passed);
            if missing > 0 {
                needed_repetitions.insert(key.clone(), missing);
                needed_pairs.insert(key);
            }
        }
    }
    if needed_pairs.is_empty() {
        return Ok(None);
    }

    let scoped_targets = target_ids
        .iter()
        .filter(|target_id| {
            needed_pairs
                .iter()
                .any(|(needed_target_id, _)| needed_target_id == *target_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let scoped_tasks = task_ids
        .iter()
        .filter(|task_id| {
            needed_pairs
                .iter()
                .any(|(_, needed_task_id)| needed_task_id == *task_id)
        })
        .cloned()
        .collect::<Vec<_>>();

    if scoped_targets.len() == target_ids.len() && scoped_tasks.len() == task_ids.len() {
        let scoped_repetitions = needed_repetitions
            .values()
            .copied()
            .max()
            .unwrap_or(repetitions)
            .min(repetitions) as u32;
        if scoped_repetitions == request.repetitions.max(1) {
            return Ok(None);
        }
        return Ok(Some(RetryScope {
            target_ids,
            task_ids,
            repetitions: scoped_repetitions,
        }));
    }

    let rectangle_matches_needed = scoped_targets.len() * scoped_tasks.len() == needed_pairs.len()
        && scoped_targets.iter().all(|target_id| {
            scoped_tasks
                .iter()
                .all(|task_id| needed_pairs.contains(&(target_id.clone(), task_id.clone())))
        });
    if !rectangle_matches_needed {
        let scoped_repetitions = needed_repetitions
            .values()
            .copied()
            .max()
            .unwrap_or(repetitions)
            .min(repetitions) as u32;
        if scoped_repetitions == request.repetitions.max(1) {
            return Ok(None);
        }
        return Ok(Some(RetryScope {
            target_ids,
            task_ids,
            repetitions: scoped_repetitions,
        }));
    }

    let scoped_repetitions = needed_repetitions
        .values()
        .copied()
        .max()
        .unwrap_or(repetitions)
        .min(repetitions) as u32;
    Ok(Some(RetryScope {
        target_ids: scoped_targets,
        task_ids: scoped_tasks,
        repetitions: scoped_repetitions.max(1),
    }))
}

fn enforce_run_cost_limit(
    request: &runner::RunQuickSmokeRequest,
    targets: &[store::TargetRecord],
) -> Result<(), String> {
    let Some(max_cost_usd) = request.max_cost_usd else {
        return Ok(());
    };
    if !max_cost_usd.is_finite() || max_cost_usd < 0.0 {
        return Err("max_cost_invalid: maxCostUsd must be a non-negative number".into());
    }

    let pack = runner::load_pack(&request.benchmark_pack_id)?;
    let tasks = runner::select_tasks_for_run(runner::load_tasks(&pack)?, &request.task_ids)?;
    let repetitions = request.repetitions.max(1);
    let warmup_runs = request.warmup_runs.min(20);
    let prompt_tokens_per_repetition: u64 =
        tasks.iter().map(|task| estimate_tokens(&task.prompt)).sum();
    let task_count = tasks.len() as u64;
    let warmup_prompt_tokens = 8_u64;
    let mut total_cost = 0_f64;
    let mut unpriced_targets = Vec::new();

    for target in targets {
        let config = target_config_value(target);
        let effective_warmup_runs = if target_supports_warmup(target) {
            warmup_runs
        } else {
            0
        };
        let target_prompt_tokens = prompt_tokens_per_repetition
            .saturating_mul(repetitions as u64)
            .saturating_add(warmup_prompt_tokens.saturating_mul(effective_warmup_runs as u64));
        let target_calls = task_count
            .saturating_mul(repetitions as u64)
            .saturating_add(effective_warmup_runs as u64);
        let target_completion_tokens =
            target_calls.saturating_mul(configured_max_tokens_for_tasks(&config, &tasks));
        match (
            price_per_million(&config, "input_price_usd_per_million_tokens")
                .or_else(|| price_per_million(&config, "input_usd_per_million_tokens")),
            price_per_million(&config, "output_price_usd_per_million_tokens")
                .or_else(|| price_per_million(&config, "output_usd_per_million_tokens")),
        ) {
            (Some(input_price), Some(output_price)) => {
                let prompt_price = conservative_prompt_price_per_million(&config, input_price);
                total_cost += ((target_prompt_tokens as f64 * prompt_price)
                    + (target_completion_tokens as f64 * output_price))
                    / 1_000_000.0;
            }
            _ if targeting::target_is_known_zero_cost_when_unpriced(
                &target.kind,
                &target.adapter_id,
                &config,
            ) => {}
            _ => unpriced_targets.push(target.id.clone()),
        }
    }

    if !unpriced_targets.is_empty() {
        return Err(format!(
            "max_cost_unpriced: maxCostUsd was set to ${:.6}, but {} selected target(s) have no pricing: {}. Add target pricing or clear the max cost limit.",
            max_cost_usd,
            unpriced_targets.len(),
            unpriced_targets.join(", ")
        ));
    }
    if total_cost > max_cost_usd {
        return Err(format!(
            "max_cost_exceeded: estimated upper-bound cost ${:.6} exceeds maxCostUsd ${:.6}. Reduce targets, repetitions, warmups, max tokens, or raise the cap.",
            total_cost, max_cost_usd
        ));
    }
    Ok(())
}

fn target_supports_warmup(target: &store::TargetRecord) -> bool {
    matches!(
        target.kind.as_str(),
        "mock" | "direct_model" | "harnessed_model"
    )
}

fn estimate_tokens(text: &str) -> u64 {
    ((text.chars().count() as u64) + 3) / 4
}

fn configured_max_tokens_for_tasks(config: &Value, tasks: &[runner::TaskSpec]) -> u64 {
    configured_max_tokens(config).unwrap_or_else(|| default_max_tokens_for_tasks(tasks))
}

fn configured_max_tokens(config: &Value) -> Option<u64> {
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

fn price_per_million(config: &Value, key: &str) -> Option<f64> {
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

fn conservative_prompt_price_per_million(config: &Value, input_price: f64) -> f64 {
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

fn run_group_targets(
    conn: &Connection,
    target_ids: &[String],
) -> Result<Vec<store::TargetRecord>, String> {
    let available_targets = store::list_targets(conn).map_err(|err| err.to_string())?;
    Ok(target_ids
        .iter()
        .filter_map(|target_id| {
            available_targets
                .iter()
                .find(|target| target.id == *target_id)
                .cloned()
        })
        .collect())
}

fn run_group_config_snapshot(
    request: &runner::RunQuickSmokeRequest,
    targets: &[store::TargetRecord],
    tasks: &[runner::TaskSpec],
    replay: Option<&RunJobReplayDto>,
) -> Value {
    let generation_defaults = generation_snapshot_defaults(tasks);
    let mut config = Map::new();
    config.insert("docker".into(), serde_json::json!(request.docker));
    config.insert("repetitions".into(), serde_json::json!(request.repetitions));
    config.insert("warmup_runs".into(), serde_json::json!(request.warmup_runs));
    config.insert("concurrency".into(), serde_json::json!(request.concurrency));
    config.insert("task_count".into(), serde_json::json!(tasks.len()));
    if !request.task_ids.is_empty() {
        config.insert("task_ids".into(), serde_json::json!(&request.task_ids));
    }
    if let Some(max_cost_usd) = request.max_cost_usd {
        config.insert("max_cost_usd".into(), serde_json::json!(max_cost_usd));
    }
    if let Some(replay) = replay {
        config.insert(
            "replay".into(),
            serde_json::json!({
                "mode": replay.mode,
                "source_job_id": replay.source_job_id,
                "source_run_group_id": replay.source_run_group_id,
                "source_target_count": replay.source_target_count,
                "source_task_count": replay.source_task_count,
                "source_repetitions": replay.source_repetitions,
                "target_count": replay.target_count,
                "task_count": replay.task_count,
                "repetitions": replay.repetitions,
                "scoped": replay.scoped,
            }),
        );
    }
    config.insert(
        "targets".into(),
        Value::Array(
            targets
                .iter()
                .map(|target| run_group_target_snapshot(target, generation_defaults))
                .collect(),
        ),
    );
    Value::Object(config)
}

fn run_group_target_snapshot(
    target: &store::TargetRecord,
    generation_defaults: GenerationSnapshotDefaults,
) -> Value {
    let config = target_config_value(target);
    let mut snapshot = Map::new();
    snapshot.insert("id".into(), Value::String(target.id.clone()));
    snapshot.insert("name".into(), Value::String(target.name.clone()));
    snapshot.insert("kind".into(), Value::String(target.kind.clone()));
    snapshot.insert(
        "adapter_id".into(),
        Value::String(target.adapter_id.clone()),
    );
    snapshot.insert("enabled".into(), Value::Bool(target.enabled));

    for key in [
        "model",
        "deployment",
        "source",
        "repo_id",
        "gguf_file",
        "context",
        "streaming",
        "reasoning_effort",
    ] {
        if let Some(value) = primitive_config_value(&config, key) {
            snapshot.insert(key.into(), value);
        }
    }
    if let Some(base_url) = config
        .get("base_url")
        .and_then(|value| value.as_str())
        .and_then(sanitized_url_snapshot)
    {
        snapshot.insert("base_url".into(), Value::String(base_url));
    }

    snapshot.insert(
        "generation".into(),
        generation_snapshot(&config, generation_defaults),
    );
    if let Some(pricing) = pricing_snapshot(&config) {
        snapshot.insert("pricing".into(), pricing);
    }
    if let Some(validation) = target_validation_snapshot(target) {
        snapshot.insert("validation".into(), validation);
    }

    Value::Object(snapshot)
}

fn target_validation_snapshot(target: &store::TargetRecord) -> Option<Value> {
    let status = target.validation_status.as_deref()?.trim();
    if status.is_empty() {
        return None;
    }

    let mut validation = Map::new();
    validation.insert("status".into(), Value::String(status.into()));
    if let Some(detail) = target
        .validation_detail
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        validation.insert("detail".into(), Value::String(detail.into()));
    }
    if let Some(checked_at) = target
        .validation_checked_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        validation.insert("checked_at".into(), Value::String(checked_at.into()));
    }
    Some(Value::Object(validation))
}

fn target_config_value(target: &store::TargetRecord) -> Value {
    serde_json::from_str(&target.config_json).unwrap_or_else(|_| serde_json::json!({}))
}

fn primitive_config_value(config: &Value, key: &str) -> Option<Value> {
    match config.get(key)? {
        Value::String(value) if !value.trim().is_empty() => Some(Value::String(value.clone())),
        Value::Number(_) | Value::Bool(_) => config.get(key).cloned(),
        _ => None,
    }
}

fn generation_snapshot_defaults(tasks: &[runner::TaskSpec]) -> GenerationSnapshotDefaults {
    let has_prompt_tasks = tasks.iter().any(|task| task.task_type == "prompt");
    let has_workspace_tasks = tasks.iter().any(|task| task.task_type != "prompt");
    GenerationSnapshotDefaults {
        default_max_tokens: if has_workspace_tasks {
            WORKSPACE_DEFAULT_MAX_TOKENS
        } else {
            PROMPT_DEFAULT_MAX_TOKENS
        },
        has_prompt_tasks,
        has_workspace_tasks,
    }
}

fn generation_snapshot(config: &Value, defaults: GenerationSnapshotDefaults) -> Value {
    let mut generation = Map::new();
    generation.insert(
        "temperature".into(),
        serde_json::json!(config_number(config, "temperature", 0.0, 0.0, 2.0)),
    );
    generation.insert(
        "top_p".into(),
        serde_json::json!(config_number(config, "top_p", 1.0, 0.0, 1.0)),
    );
    let configured_max_tokens = config_u64_value(config, "max_tokens", 1, u64::MAX);
    generation.insert(
        "max_tokens".into(),
        serde_json::json!(configured_max_tokens.unwrap_or(defaults.default_max_tokens)),
    );
    generation.insert(
        "max_tokens_source".into(),
        Value::String(
            if configured_max_tokens.is_some() {
                "target_config"
            } else {
                "runner_default"
            }
            .into(),
        ),
    );
    if configured_max_tokens.is_none() && defaults.has_workspace_tasks {
        generation.insert(
            "default_max_tokens_by_task_type".into(),
            default_max_tokens_by_task_type(defaults),
        );
    }
    generation.insert(
        "timeout_seconds".into(),
        serde_json::json!(config_u64(config, "timeout_seconds", 120, 1, 3_600)),
    );
    generation.insert(
        "retry_count".into(),
        serde_json::json!(config_u64(config, "retry_count", 1, 0, 5)),
    );
    if let Some(seed) = config_i64(config, "seed") {
        generation.insert("seed".into(), serde_json::json!(seed));
    }
    Value::Object(generation)
}

fn default_max_tokens_by_task_type(defaults: GenerationSnapshotDefaults) -> Value {
    let mut by_task_type = Map::new();
    if defaults.has_prompt_tasks {
        by_task_type.insert(
            "prompt".into(),
            serde_json::json!(PROMPT_DEFAULT_MAX_TOKENS),
        );
    }
    if defaults.has_workspace_tasks {
        by_task_type.insert(
            "workspace".into(),
            serde_json::json!(WORKSPACE_DEFAULT_MAX_TOKENS),
        );
    }
    Value::Object(by_task_type)
}

fn pricing_snapshot(config: &Value) -> Option<Value> {
    let mut pricing = Map::new();
    for key in [
        "input_price_usd_per_million_tokens",
        "output_price_usd_per_million_tokens",
        "input_usd_per_million_tokens",
        "output_usd_per_million_tokens",
        "cache_read_price_usd_per_million_tokens",
        "cache_write_price_usd_per_million_tokens",
        "cached_input_price_usd_per_million_tokens",
        "cache_creation_price_usd_per_million_tokens",
        "pricing_provider",
        "pricing_preset",
        "pricing_source",
        "pricing_verified_at",
        "pricing_note",
    ] {
        if let Some(value) = primitive_config_value(config, key) {
            pricing.insert(key.into(), value);
        }
    }
    (!pricing.is_empty()).then_some(Value::Object(pricing))
}

fn config_number(config: &Value, key: &str, default: f64, min: f64, max: f64) -> f64 {
    config
        .get(key)
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value >= min && *value <= max)
        .unwrap_or(default)
}

fn config_u64(config: &Value, key: &str, default: u64, min: u64, max: u64) -> u64 {
    config_u64_value(config, key, min, max).unwrap_or(default)
}

fn config_u64_value(config: &Value, key: &str, min: u64, max: u64) -> Option<u64> {
    config
        .get(key)
        .and_then(|value| value.as_u64())
        .filter(|value| *value >= min)
        .map(|value| value.clamp(min, max))
}

fn config_i64(config: &Value, key: &str) -> Option<i64> {
    config.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|seed| i64::try_from(seed).ok()))
    })
}

fn sanitized_url_snapshot(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let sanitized = if let Some(scheme_index) = without_query.find("://") {
        let authority_start = scheme_index + 3;
        if let Some(at_index) = without_query[authority_start..].find('@') {
            let at_index = authority_start + at_index;
            format!(
                "{}{}",
                &without_query[..authority_start],
                &without_query[at_index + 1..]
            )
        } else {
            without_query.to_string()
        }
    } else {
        without_query.to_string()
    };
    (!sanitized.trim().is_empty()).then_some(sanitized)
}

fn cancellation_requested(conn: &Connection, id: &str) -> bool {
    store::run_job_cancellation_requested(conn, id).unwrap_or(false)
}

fn finish_cancelled(conn: &Connection, job_id: &str, group_id: &str) {
    let finished_at = store::now();
    let (total, completed) = current_counts(conn, job_id);
    let result_ids = store::list_results_for_group(conn, group_id)
        .unwrap_or_default()
        .into_iter()
        .map(|result| result.id)
        .collect::<Vec<_>>();
    let _ = store::finish_run_job(
        conn,
        job_id,
        "cancelled",
        "Cancelled by user",
        total,
        completed,
        &finished_at,
        Some("cancelled"),
        &result_ids,
    );
    let _ = store::update_run_group_status(conn, group_id, "cancelled", Some(&finished_at));
}

fn finish_failed_with_partial_results(
    conn: &Connection,
    job_id: &str,
    group_id: &str,
    err: &str,
    finished_at: &str,
) {
    let (total, completed) = current_counts(conn, job_id);
    let result_ids = store::list_results_for_group(conn, group_id)
        .unwrap_or_default()
        .into_iter()
        .map(|result| result.id)
        .collect::<Vec<_>>();
    let completed = completed.max(result_ids.len());
    let message = if result_ids.is_empty() {
        err.to_string()
    } else {
        format!("{} ({} partial result(s) available)", err, result_ids.len())
    };
    let _ = store::finish_run_job(
        conn,
        job_id,
        "failed",
        &message,
        total,
        completed,
        finished_at,
        Some(err),
        &result_ids,
    );
    let _ = store::update_run_group_status(conn, group_id, "failed", Some(finished_at));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retry_test_job(request: serde_json::Value) -> store::RunJobRecord {
        store::RunJobRecord {
            id: "retry-job".into(),
            run_group_id: "retry-group".into(),
            benchmark_pack_id: request
                .get("benchmarkPackId")
                .and_then(Value::as_str)
                .unwrap_or("llm-basics")
                .into(),
            status: "completed".into(),
            message: String::new(),
            started_at: "2026-01-01T00:00:00Z".into(),
            finished_at: Some("2026-01-01T00:00:01Z".into()),
            total: 0,
            completed: 0,
            error: None,
            request,
            result_run_ids: vec![],
        }
    }

    fn retry_test_result(
        id: &str,
        target_id: &str,
        task_id: &str,
        status: &str,
    ) -> store::ResultRecord {
        store::ResultRecord {
            id: id.into(),
            run_group_id: Some("retry-group".into()),
            target_id: target_id.into(),
            benchmark_pack_id: "llm-basics".into(),
            task_id: task_id.into(),
            status: status.into(),
            started_at: Some("2026-01-01T00:00:00Z".into()),
            finished_at: Some("2026-01-01T00:00:01Z".into()),
            pass_fail: Some(status == "passed"),
            score: if status == "passed" {
                Some(1.0)
            } else {
                Some(0.0)
            },
            score_numeric: if status == "passed" {
                Some(1.0)
            } else {
                Some(0.0)
            },
            wall_time_ms: Some(100.0),
            setup_time_ms: None,
            target_time_ms: None,
            evaluation_time_ms: None,
            model_call_wall_time_ms: None,
            input_tokens: None,
            output_tokens: None,
            prompt_tokens: None,
            completion_tokens: None,
            reasoning_tokens: None,
            cached_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: None,
            estimated_cost_usd: None,
            cost_usd: None,
            provider_attempts: None,
            provider_retry_after_ms: None,
            provider_retry_delay_ms: None,
            http_status: None,
            provider_time_to_first_byte_ms: None,
            ttft_ms: None,
            provider_time_to_first_token_ms: None,
            provider_request_total_ms: None,
            decode_tokens_per_sec: None,
            output_tokens_per_second: None,
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
            import_truncated: None,
            import_truncated_bytes: None,
            provider_model: None,
            provider_model_source: None,
            finish_reason: None,
            pricing_assumption: None,
            import_format: None,
            import_source: None,
            import_path: None,
            summary_source: None,
            error_code: None,
            error_message: None,
            reproducibility: serde_json::json!({}),
        }
    }

    #[test]
    fn job_store_lists_newest_first() {
        let conn = store::open_memory().unwrap();
        for (id, group_id, started_at) in [
            ("old", "old-group", "2026-01-01T00:00:00Z"),
            ("new", "new-group", "2026-01-02T00:00:00Z"),
        ] {
            store::insert_run_group(
                &conn,
                group_id,
                "quick-smoke",
                &["mock-agent".into()],
                "queued",
                started_at,
                &serde_json::json!({}),
            )
            .unwrap();
            store::insert_run_job(
                &conn,
                &store::RunJobRecord {
                    id: id.into(),
                    run_group_id: group_id.into(),
                    benchmark_pack_id: "quick-smoke".into(),
                    status: "queued".into(),
                    message: String::new(),
                    started_at: started_at.into(),
                    finished_at: None,
                    total: 0,
                    completed: 0,
                    error: None,
                    request: serde_json::json!({}),
                    result_run_ids: vec![],
                },
            )
            .unwrap();
        }

        let jobs = list_jobs(&conn).unwrap();
        assert_eq!(jobs[0].id, "new");
        assert_eq!(jobs[0].run_group_id, "new-group");
    }

    #[test]
    fn cancel_job_marks_running_job_cancelling() {
        let conn = store::open_memory().unwrap();
        store::insert_run_group(
            &conn,
            "cancel-group",
            "quick-smoke",
            &["mock-agent".into()],
            "running",
            "2026-01-01T00:00:00Z",
            &serde_json::json!({}),
        )
        .unwrap();
        store::insert_run_job(
            &conn,
            &store::RunJobRecord {
                id: "cancel-me".into(),
                run_group_id: "cancel-group".into(),
                benchmark_pack_id: "quick-smoke".into(),
                status: "running".into(),
                message: "Running".into(),
                started_at: "2026-01-01T00:00:00Z".into(),
                finished_at: None,
                total: 2,
                completed: 1,
                error: None,
                request: serde_json::json!({}),
                result_run_ids: vec![],
            },
        )
        .unwrap();

        let job = cancel_job(&conn, "cancel-me").unwrap().unwrap();
        assert_eq!(job.status, "cancelling");
        assert!(cancellation_requested(&conn, "cancel-me"));
    }

    #[test]
    fn replay_rejects_active_jobs_even_with_partial_failures() {
        let conn = store::open_memory().unwrap();
        store::insert_run_group(
            &conn,
            "active-group",
            "llm-basics",
            &["mock-agent".into()],
            "running",
            "2026-01-01T00:00:00Z",
            &serde_json::json!({}),
        )
        .unwrap();
        store::insert_run_job(
            &conn,
            &store::RunJobRecord {
                id: "active-job".into(),
                run_group_id: "active-group".into(),
                benchmark_pack_id: "llm-basics".into(),
                status: "running".into(),
                message: "Running prompt".into(),
                started_at: "2026-01-01T00:00:00Z".into(),
                finished_at: None,
                total: 3,
                completed: 1,
                error: None,
                request: serde_json::json!({
                    "targetIds": ["mock-agent"],
                    "benchmarkPackId": "llm-basics",
                    "repetitions": 1,
                    "warmupRuns": 0,
                    "concurrency": 1,
                    "docker": false
                }),
                result_run_ids: vec![],
            },
        )
        .unwrap();
        store::insert_run_with_group(
            &conn,
            "partial-failure",
            Some("active-group"),
            "mock-agent",
            "llm-basics",
            "prompt-json",
            "failed",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:01Z",
            Some("test_failed"),
            Some("JSON expectation failed"),
            &serde_json::json!({}),
            &serde_json::json!({}),
        )
        .unwrap();

        let duplicate_err =
            duplicate_job(&conn, "active-job").expect_err("active jobs should not duplicate");
        assert!(duplicate_err.contains("active_job_replay_blocked"));
        let retry_err = retry_job(&conn, "active-job").expect_err("active jobs should not retry");
        assert!(retry_err.contains("active_job_replay_blocked"));
        assert_eq!(store::list_run_jobs(&conn).unwrap().len(), 1);
    }

    #[test]
    fn failed_job_preserves_partial_results() {
        let conn = store::open_memory().unwrap();
        let started_at = "2026-01-01T00:00:00Z";
        store::insert_run_group(
            &conn,
            "partial-group",
            "llm-basics",
            &["mock-agent".into()],
            "running",
            started_at,
            &serde_json::json!({}),
        )
        .unwrap();
        store::insert_run_job(
            &conn,
            &store::RunJobRecord {
                id: "partial-job".into(),
                run_group_id: "partial-group".into(),
                benchmark_pack_id: "llm-basics".into(),
                status: "running".into(),
                message: "Running".into(),
                started_at: started_at.into(),
                finished_at: None,
                total: 3,
                completed: 0,
                error: None,
                request: serde_json::json!({}),
                result_run_ids: vec![],
            },
        )
        .unwrap();
        store::insert_run_with_group(
            &conn,
            "partial-run",
            Some("partial-group"),
            "mock-agent",
            "llm-basics",
            "instruction-following",
            "passed",
            started_at,
            started_at,
            None,
            None,
            &serde_json::json!({}),
            &serde_json::json!({}),
        )
        .unwrap();

        finish_failed_with_partial_results(
            &conn,
            "partial-job",
            "partial-group",
            "provider_failed: timeout",
            "2026-01-01T00:01:00Z",
        );

        let record = store::get_run_job(&conn, "partial-job").unwrap().unwrap();
        assert_eq!(record.status, "failed");
        assert_eq!(record.completed, 1);
        assert_eq!(record.error.as_deref(), Some("provider_failed: timeout"));
        assert_eq!(record.result_run_ids, vec!["partial-run".to_string()]);
        assert!(record.message.contains("1 partial result"));

        let dto = get_job(&conn, "partial-job").unwrap().unwrap();
        assert_eq!(dto.results.len(), 1);
        assert_eq!(dto.results[0].id, "partial-run");
    }

    #[test]
    fn request_from_job_drops_old_group_id() {
        let job = store::RunJobRecord {
            id: "job".into(),
            run_group_id: "group".into(),
            benchmark_pack_id: "quick-smoke".into(),
            status: "completed".into(),
            message: String::new(),
            started_at: "2026-01-01T00:00:00Z".into(),
            finished_at: None,
            total: 0,
            completed: 0,
            error: None,
            request: serde_json::json!({
                "targetIds": ["mock-agent"],
                "benchmarkPackId": "quick-smoke",
                "repetitions": 0,
                "warmupRuns": 99,
                "concurrency": 99,
                "maxCostUsd": 0.25,
                "docker": false,
                "runGroupId": "old-group"
            }),
            result_run_ids: vec![],
        };
        let request = request_from_job(&job).unwrap();
        assert_eq!(request.run_group_id, None);
        assert_eq!(request.repetitions, 1);
        assert_eq!(request.warmup_runs, 20);
        assert_eq!(request.concurrency, 8);
        assert_eq!(request.max_cost_usd, Some(0.25));
        assert_eq!(request.target_ids, vec!["mock-agent"]);
    }

    #[test]
    fn retry_request_scopes_to_single_failed_task_slot() {
        let job = retry_test_job(serde_json::json!({
            "targetIds": ["target-a", "target-b"],
            "benchmarkPackId": "llm-basics",
            "taskIds": ["llm-instruction-following-001", "llm-json-validity-001"],
            "repetitions": 1,
            "warmupRuns": 0,
            "concurrency": 1,
            "docker": false
        }));
        let request = retry_request_from_job_results(
            &job,
            &[
                retry_test_result(
                    "a-instruction",
                    "target-a",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result("a-json", "target-a", "llm-json-validity-001", "failed"),
                retry_test_result(
                    "b-instruction",
                    "target-b",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result("b-json", "target-b", "llm-json-validity-001", "passed"),
            ],
        )
        .expect("retry request should build");

        assert_eq!(request.target_ids, vec!["target-a"]);
        assert_eq!(request.task_ids, vec!["llm-json-validity-001"]);
        assert_eq!(request.repetitions, 1);
    }

    #[test]
    fn retry_request_scopes_to_missing_target_rectangle() {
        let job = retry_test_job(serde_json::json!({
            "targetIds": ["target-a", "target-b"],
            "benchmarkPackId": "llm-basics",
            "taskIds": ["llm-instruction-following-001", "llm-json-validity-001"],
            "repetitions": 1,
            "warmupRuns": 0,
            "concurrency": 1,
            "docker": false
        }));
        let request = retry_request_from_job_results(
            &job,
            &[
                retry_test_result(
                    "a-instruction",
                    "target-a",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result("a-json", "target-a", "llm-json-validity-001", "passed"),
            ],
        )
        .expect("retry request should build");

        assert_eq!(request.target_ids, vec!["target-b"]);
        assert_eq!(
            request.task_ids,
            vec!["llm-instruction-following-001", "llm-json-validity-001"]
        );
    }

    #[test]
    fn retry_request_keeps_full_scope_for_diagonal_failures() {
        let job = retry_test_job(serde_json::json!({
            "targetIds": ["target-a", "target-b"],
            "benchmarkPackId": "llm-basics",
            "taskIds": ["llm-instruction-following-001", "llm-json-validity-001"],
            "repetitions": 1,
            "warmupRuns": 0,
            "concurrency": 1,
            "docker": false
        }));
        let request = retry_request_from_job_results(
            &job,
            &[
                retry_test_result(
                    "a-instruction",
                    "target-a",
                    "llm-instruction-following-001",
                    "failed",
                ),
                retry_test_result("a-json", "target-a", "llm-json-validity-001", "passed"),
                retry_test_result(
                    "b-instruction",
                    "target-b",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result("b-json", "target-b", "llm-json-validity-001", "failed"),
            ],
        )
        .expect("retry request should build");

        assert_eq!(request.target_ids, vec!["target-a", "target-b"]);
        assert_eq!(
            request.task_ids,
            vec!["llm-instruction-following-001", "llm-json-validity-001"]
        );
    }

    #[test]
    fn retry_request_reduces_repetitions_for_diagonal_incomplete_slots() {
        let job = retry_test_job(serde_json::json!({
            "targetIds": ["target-a", "target-b"],
            "benchmarkPackId": "llm-basics",
            "taskIds": ["llm-instruction-following-001", "llm-json-validity-001"],
            "repetitions": 3,
            "warmupRuns": 1,
            "concurrency": 2,
            "docker": false
        }));
        let mut results = Vec::new();
        for repetition in 0..2 {
            results.push(retry_test_result(
                &format!("a-instruction-{repetition}"),
                "target-a",
                "llm-instruction-following-001",
                "passed",
            ));
            results.push(retry_test_result(
                &format!("b-json-{repetition}"),
                "target-b",
                "llm-json-validity-001",
                "passed",
            ));
        }
        for repetition in 0..3 {
            results.push(retry_test_result(
                &format!("a-json-{repetition}"),
                "target-a",
                "llm-json-validity-001",
                "passed",
            ));
            results.push(retry_test_result(
                &format!("b-instruction-{repetition}"),
                "target-b",
                "llm-instruction-following-001",
                "passed",
            ));
        }
        let request =
            retry_request_from_job_results(&job, &results).expect("retry request should build");

        assert_eq!(request.target_ids, vec!["target-a", "target-b"]);
        assert_eq!(
            request.task_ids,
            vec!["llm-instruction-following-001", "llm-json-validity-001"]
        );
        assert_eq!(request.repetitions, 1);
    }

    #[test]
    fn retry_request_scopes_incomplete_repetitions_to_remaining_count() {
        let job = retry_test_job(serde_json::json!({
            "targetIds": ["target-a", "target-b"],
            "benchmarkPackId": "llm-basics",
            "taskIds": ["llm-instruction-following-001", "llm-json-validity-001"],
            "repetitions": 3,
            "warmupRuns": 1,
            "concurrency": 2,
            "docker": false
        }));
        let request = retry_request_from_job_results(
            &job,
            &[
                retry_test_result(
                    "a-instruction-1",
                    "target-a",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result(
                    "a-instruction-2",
                    "target-a",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result("a-json-1", "target-a", "llm-json-validity-001", "passed"),
                retry_test_result("a-json-2", "target-a", "llm-json-validity-001", "passed"),
                retry_test_result("a-json-3", "target-a", "llm-json-validity-001", "passed"),
                retry_test_result(
                    "b-instruction-1",
                    "target-b",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result(
                    "b-instruction-2",
                    "target-b",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result(
                    "b-instruction-3",
                    "target-b",
                    "llm-instruction-following-001",
                    "passed",
                ),
                retry_test_result("b-json-1", "target-b", "llm-json-validity-001", "passed"),
                retry_test_result("b-json-2", "target-b", "llm-json-validity-001", "passed"),
                retry_test_result("b-json-3", "target-b", "llm-json-validity-001", "passed"),
            ],
        )
        .expect("retry request should build");

        assert_eq!(request.target_ids, vec!["target-a"]);
        assert_eq!(request.task_ids, vec!["llm-instruction-following-001"]);
        assert_eq!(request.repetitions, 1);
        assert_eq!(request.warmup_runs, 1);
        assert_eq!(request.concurrency, 2);
    }

    #[test]
    fn retry_request_reduces_repetitions_without_narrowing_target_or_task_scope() {
        let job = retry_test_job(serde_json::json!({
            "targetIds": ["target-a", "target-b"],
            "benchmarkPackId": "llm-basics",
            "taskIds": ["llm-instruction-following-001", "llm-json-validity-001"],
            "repetitions": 3,
            "warmupRuns": 1,
            "concurrency": 2,
            "docker": false
        }));
        let mut results = Vec::new();
        for target_id in ["target-a", "target-b"] {
            for task_id in ["llm-instruction-following-001", "llm-json-validity-001"] {
                for repetition in 0..2 {
                    results.push(retry_test_result(
                        &format!("{target_id}-{task_id}-{repetition}"),
                        target_id,
                        task_id,
                        "passed",
                    ));
                }
            }
        }
        let request =
            retry_request_from_job_results(&job, &results).expect("retry request should build");

        assert_eq!(request.target_ids, vec!["target-a", "target-b"]);
        assert_eq!(
            request.task_ids,
            vec!["llm-instruction-following-001", "llm-json-validity-001"]
        );
        assert_eq!(request.repetitions, 1);
        assert_eq!(request.warmup_runs, 1);
        assert_eq!(request.concurrency, 2);
    }

    #[test]
    fn job_dto_exposes_replay_settings_summary() {
        let conn = store::open_memory().unwrap();
        store::insert_run_group(
            &conn,
            "settings-group",
            "llm-basics",
            &["mock-agent".into(), "local-agent".into()],
            "completed",
            "2026-01-01T00:00:00Z",
            &serde_json::json!({}),
        )
        .unwrap();
        store::insert_run_job(
            &conn,
            &store::RunJobRecord {
                id: "settings-job".into(),
                run_group_id: "settings-group".into(),
                benchmark_pack_id: "llm-basics".into(),
                status: "completed".into(),
                message: String::new(),
                started_at: "2026-01-01T00:00:00Z".into(),
                finished_at: Some("2026-01-01T00:00:01Z".into()),
                total: 6,
                completed: 6,
                error: None,
                request: serde_json::json!({
                    "targetIds": ["mock-agent", "local-agent"],
                    "benchmarkPackId": "llm-basics",
                    "repetitions": 3,
                    "warmupRuns": 1,
                    "concurrency": 2,
                    "docker": true,
                    "maxCostUsd": 0.25,
                    "replay": {
                        "mode": "retry",
                        "sourceJobId": "source-job",
                        "sourceRunGroupId": "source-group",
                        "sourceTargetCount": 3,
                        "sourceTaskCount": 4,
                        "sourceRepetitions": 3,
                        "targetCount": 2,
                        "taskCount": 3,
                        "repetitions": 1,
                        "scoped": true
                    }
                }),
                result_run_ids: vec![],
            },
        )
        .unwrap();

        let job = get_job(&conn, "settings-job").unwrap().unwrap();

        assert_eq!(job.settings.target_count, 2);
        assert_eq!(job.settings.repetitions, 3);
        assert_eq!(job.settings.warmup_runs, 1);
        assert_eq!(job.settings.concurrency, 2);
        assert!(job.settings.docker);
        assert_eq!(job.settings.max_cost_usd, Some(0.25));
        let replay = job.settings.replay.expect("replay settings should parse");
        assert_eq!(replay.mode, "retry");
        assert_eq!(replay.source_job_id, "source-job");
        assert_eq!(replay.source_run_group_id, "source-group");
        assert_eq!(replay.source_target_count, 3);
        assert_eq!(replay.source_task_count, 4);
        assert_eq!(replay.source_repetitions, 3);
        assert_eq!(replay.target_count, 2);
        assert_eq!(replay.task_count, 3);
        assert_eq!(replay.repetitions, 1);
        assert!(replay.scoped);
    }

    #[test]
    fn run_group_config_records_replay_metadata() {
        let tasks = runner::select_tasks_for_run(
            runner::load_tasks(&runner::load_pack("llm-basics").unwrap()).unwrap(),
            &[
                "llm-instruction-following-001".into(),
                "llm-json-validity-001".into(),
            ],
        )
        .unwrap();
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["mock-agent".into()],
            benchmark_pack_id: "llm-basics".into(),
            task_ids: vec![
                "llm-instruction-following-001".into(),
                "llm-json-validity-001".into(),
            ],
            repetitions: 1,
            docker: false,
            warmup_runs: 1,
            concurrency: 1,
            max_cost_usd: Some(0.25),
            run_group_id: Some("retry-group".into()),
        };
        let replay = RunJobReplayDto {
            mode: "retry".into(),
            source_job_id: "source-job".into(),
            source_run_group_id: "source-group".into(),
            source_target_count: 2,
            source_task_count: 2,
            source_repetitions: 3,
            target_count: 1,
            task_count: 2,
            repetitions: 1,
            scoped: true,
        };
        let snapshot = run_group_config_snapshot(&request, &[], &tasks, Some(&replay));

        assert_eq!(snapshot["replay"]["mode"], "retry");
        assert_eq!(snapshot["replay"]["source_job_id"], "source-job");
        assert_eq!(snapshot["replay"]["source_run_group_id"], "source-group");
        assert_eq!(snapshot["replay"]["source_target_count"], 2);
        assert_eq!(snapshot["replay"]["source_task_count"], 2);
        assert_eq!(snapshot["replay"]["source_repetitions"], 3);
        assert_eq!(snapshot["replay"]["target_count"], 1);
        assert_eq!(snapshot["replay"]["task_count"], 2);
        assert_eq!(snapshot["replay"]["repetitions"], 1);
        assert_eq!(snapshot["replay"]["scoped"], true);
    }

    #[test]
    fn start_job_enforces_max_cost_for_replays_and_direct_queue_starts() {
        let conn = store::open_memory().unwrap();
        let api_key_env = format!(
            "BENCHFORGE_TEST_REMOTE_COMPATIBLE_KEY_{}",
            uuid::Uuid::new_v4()
        )
        .replace('-', "_");
        std::env::set_var(&api_key_env, "benchforge-test-key");
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
                    "api_key_env": api_key_env
                }),
            },
        )
        .unwrap();
        let request = runner::RunQuickSmokeRequest {
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

        let err = start_quick_smoke_job(&conn, request)
            .err()
            .expect("unpriced capped job should be rejected");
        assert!(err.starts_with("max_cost_unpriced"), "{err}");
        let jobs = store::list_run_jobs(&conn).expect("jobs should list");
        assert!(jobs.is_empty(), "rejected capped job should not be queued");
    }

    #[test]
    fn queued_cost_limit_allows_unpriced_local_targets_as_zero_cost() {
        let conn = store::open_memory().unwrap();
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
        .unwrap();
        let target = store::get_target(&conn, "manual-local").unwrap().unwrap();
        let request = runner::RunQuickSmokeRequest {
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

        enforce_run_cost_limit(&request, &[target])
            .expect("unpriced localhost target should count as known zero cost");
    }

    #[test]
    fn queued_cost_limit_uses_prompt_default_for_prompt_only_packs() {
        let conn = store::open_memory().unwrap();
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "priced-prompt-target".into(),
                name: "Priced Prompt Target".into(),
                kind: "direct_model".into(),
                adapter_id: "openai-compatible".into(),
                config: serde_json::json!({
                    "model": "local",
                    "base_url": "http://127.0.0.1:8080/v1",
                    "input_price_usd_per_million_tokens": 0.0,
                    "output_price_usd_per_million_tokens": 1000.0
                }),
            },
        )
        .unwrap();
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["priced-prompt-target".into()],
            benchmark_pack_id: "llm-basics".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: Some(2.0),
            run_group_id: None,
        };

        enforce_run_cost_limit(
            &request,
            &[store::get_target(&conn, "priced-prompt-target")
                .unwrap()
                .unwrap()],
        )
        .expect("prompt-only cost cap should use the 512 token default");
    }

    #[test]
    fn queued_cost_limit_ignores_warmups_for_non_model_targets() {
        let pack = runner::load_pack("llm-connectivity").expect("pack should load");
        let task_count = runner::load_tasks(&pack).expect("tasks should load").len() as f64;
        let target = store::TargetRecord {
            id: "priced-cli".into(),
            name: "Priced CLI".into(),
            kind: "cli_agent".into(),
            adapter_id: "codex".into(),
            config_json: serde_json::json!({
                "max_tokens": 1,
                "input_price_usd_per_million_tokens": 0.0,
                "output_price_usd_per_million_tokens": 1_000_000.0
            })
            .to_string(),
            enabled: true,
            validation_status: Some("ok".into()),
            validation_detail: None,
            validation_checked_at: None,
        };
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["priced-cli".into()],
            benchmark_pack_id: "llm-connectivity".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 20,
            concurrency: 1,
            max_cost_usd: Some(task_count + 0.5),
            run_group_id: None,
        };

        enforce_run_cost_limit(&request, &[target])
            .expect("non-model warmups should not affect queued cost cap");
    }

    #[test]
    fn queued_cost_limit_uses_cache_write_price_for_prompt_bound() {
        let target = store::TargetRecord {
            id: "cache-priced-target".into(),
            name: "Cache Priced Target".into(),
            kind: "direct_model".into(),
            adapter_id: "openai-compatible".into(),
            config_json: serde_json::json!({
                "model": "local",
                "base_url": "http://127.0.0.1:8080/v1",
                "max_tokens": 1,
                "input_price_usd_per_million_tokens": 1.0,
                "output_price_usd_per_million_tokens": 0.0,
                "cache_write_price_usd_per_million_tokens": 10.0
            })
            .to_string(),
            enabled: true,
            validation_status: Some("ok".into()),
            validation_detail: None,
            validation_checked_at: None,
        };
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["cache-priced-target".into()],
            benchmark_pack_id: "llm-connectivity".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: Some(0.000001),
            run_group_id: None,
        };

        assert!(enforce_run_cost_limit(&request, &[target])
            .expect_err("cache-write priced target should exceed tiny cap")
            .starts_with("max_cost_exceeded"));
    }

    #[test]
    fn start_job_preflights_external_harness_commands() {
        let conn = store::open_memory().unwrap();
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "external-worker".into(),
                name: "External Worker".into(),
                kind: "benchmark_harness".into(),
                adapter_id: "benchforge-worker".into(),
                config: serde_json::json!({}),
            },
        )
        .unwrap();
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["external-worker".into()],
            benchmark_pack_id: "evalplus".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: None,
        };

        let err = start_quick_smoke_job(&conn, request)
            .err()
            .expect("external harness without command should not queue");

        assert!(err.contains("target_preflight_failed"), "{err}");
        assert!(err.contains("configuration_missing"), "{err}");
        assert!(err.contains("evalplus"), "{err}");
        let jobs = store::list_run_jobs(&conn).expect("jobs should list");
        assert!(jobs.is_empty(), "rejected harness job should not be queued");
    }

    #[test]
    fn start_job_rejects_targets_with_remembered_validation_errors() {
        let conn = store::open_memory().unwrap();
        store::upsert_target(
            &conn,
            &store::NewTarget {
                id: "stale-local".into(),
                name: "Stale Local".into(),
                kind: "direct_model".into(),
                adapter_id: "llama-cpp-openai".into(),
                config: serde_json::json!({
                    "model": "local",
                    "base_url": "http://127.0.0.1:8080/v1"
                }),
            },
        )
        .unwrap();
        store::set_target_validation(
            &conn,
            "stale-local",
            "error",
            "endpoint_unreachable: connection refused",
            "2026-07-07T12:00:00Z",
        )
        .unwrap();
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["stale-local".into()],
            benchmark_pack_id: "llm-connectivity".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 1,
            max_cost_usd: None,
            run_group_id: None,
        };

        let err = start_quick_smoke_job(&conn, request)
            .err()
            .expect("known-bad targets should not queue");

        assert!(err.contains("target_preflight_failed"), "{err}");
        assert!(err.contains("target_validation_failed"), "{err}");
        assert!(err.contains("endpoint_unreachable"), "{err}");
        let jobs = store::list_run_jobs(&conn).expect("jobs should list");
        assert!(
            jobs.is_empty(),
            "rejected validation-error job should not be queued"
        );
    }

    #[test]
    fn run_group_config_snapshots_target_pricing_and_generation_without_secrets() {
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["cloud".into()],
            benchmark_pack_id: "llm-core".into(),
            task_ids: vec![],
            repetitions: 3,
            docker: false,
            warmup_runs: 1,
            concurrency: 2,
            max_cost_usd: Some(0.5),
            run_group_id: Some("group".into()),
        };
        let target = store::TargetRecord {
            id: "cloud".into(),
            name: "Cloud Model".into(),
            kind: "direct_model".into(),
            adapter_id: "openai-compatible".into(),
            enabled: true,
            validation_status: Some("ok".into()),
            validation_detail: Some("completion probe succeeded; model listed".into()),
            validation_checked_at: Some("2026-07-07T12:00:00Z".into()),
            config_json: serde_json::json!({
                "model": "gpt-5-mini",
                "base_url": "https://user:topsecret@example.test/v1?api_key=leak#frag",
                "api_key": "sk-secret",
                "api_key_env": "OPENAI_API_KEY",
                "api_key_keychain": "benchforge/openai/default",
                "temperature": 0.2,
                "top_p": 0.9,
                "max_tokens": 128,
                "seed": 42,
                "timeout_seconds": 30,
                "retry_count": 2,
                "input_price_usd_per_million_tokens": 0.25,
                "output_price_usd_per_million_tokens": 2.0,
                "cache_read_price_usd_per_million_tokens": 0.025,
                "cache_write_price_usd_per_million_tokens": 1.25,
                "pricing_provider": "openai",
                "pricing_source": "provider pricing page",
                "pricing_verified_at": "2026-07-06"
            })
            .to_string(),
        };

        let pack = runner::load_pack(&request.benchmark_pack_id).expect("pack should load");
        let tasks = runner::load_tasks(&pack).expect("tasks should load");
        let snapshot = run_group_config_snapshot(&request, &[target], &tasks, None);

        assert_eq!(snapshot["repetitions"], 3);
        assert_eq!(snapshot["max_cost_usd"], 0.5);
        assert_eq!(snapshot["targets"][0]["model"], "gpt-5-mini");
        assert_eq!(
            snapshot["targets"][0]["base_url"],
            "https://example.test/v1"
        );
        assert_eq!(snapshot["targets"][0]["generation"]["temperature"], 0.2);
        assert_eq!(
            snapshot["targets"][0]["generation"]["max_tokens_source"],
            "target_config"
        );
        assert_eq!(snapshot["targets"][0]["generation"]["seed"], 42);
        assert_eq!(
            snapshot["targets"][0]["pricing"]["input_price_usd_per_million_tokens"],
            0.25
        );
        assert_eq!(
            snapshot["targets"][0]["pricing"]["cache_read_price_usd_per_million_tokens"],
            0.025
        );
        assert_eq!(
            snapshot["targets"][0]["pricing"]["cache_write_price_usd_per_million_tokens"],
            1.25
        );
        assert_eq!(
            snapshot["targets"][0]["pricing"]["pricing_verified_at"],
            "2026-07-06"
        );
        assert_eq!(snapshot["targets"][0]["validation"]["status"], "ok");
        assert_eq!(
            snapshot["targets"][0]["validation"]["detail"],
            "completion probe succeeded; model listed"
        );
        assert_eq!(
            snapshot["targets"][0]["validation"]["checked_at"],
            "2026-07-07T12:00:00Z"
        );
        let serialized = serde_json::to_string(&snapshot).unwrap();
        for forbidden in [
            "sk-secret",
            "OPENAI_API_KEY",
            "api_key_keychain",
            "benchforge/openai/default",
            "topsecret",
            "api_key=leak",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "snapshot should not contain {forbidden}"
            );
        }
    }

    #[test]
    fn run_group_config_records_pack_default_max_tokens_for_workspace_tasks() {
        let request = runner::RunQuickSmokeRequest {
            target_ids: vec!["mock-agent".into()],
            benchmark_pack_id: "quick-smoke".into(),
            task_ids: vec![],
            repetitions: 1,
            docker: false,
            warmup_runs: 0,
            concurrency: 2,
            max_cost_usd: None,
            run_group_id: None,
        };
        let target = store::TargetRecord {
            id: "mock-agent".into(),
            name: "Mock Agent".into(),
            kind: "mock".into(),
            adapter_id: "mock".into(),
            enabled: true,
            validation_status: None,
            validation_detail: None,
            validation_checked_at: None,
            config_json: serde_json::json!({"mode": "deterministic-fixture-fix"}).to_string(),
        };

        let pack = runner::load_pack(&request.benchmark_pack_id).expect("pack should load");
        let tasks = runner::load_tasks(&pack).expect("tasks should load");
        let snapshot = run_group_config_snapshot(&request, &[target], &tasks, None);

        assert_eq!(snapshot["targets"][0]["generation"]["max_tokens"], 4096);
        assert_eq!(
            snapshot["targets"][0]["generation"]["max_tokens_source"],
            "runner_default"
        );
        assert_eq!(
            snapshot["targets"][0]["generation"]["default_max_tokens_by_task_type"]["workspace"],
            4096
        );
    }

    #[test]
    fn clear_finished_jobs_keeps_running_job() {
        let conn = store::open_memory().unwrap();
        for (id, group_id, status) in [
            ("completed", "group-completed", "completed"),
            ("failed", "group-failed", "failed"),
            ("cancelled", "group-cancelled", "cancelled"),
            ("running", "group-running", "running"),
        ] {
            store::insert_run_group(
                &conn,
                group_id,
                "quick-smoke",
                &["mock-agent".into()],
                status,
                "2026-01-01T00:00:00Z",
                &serde_json::json!({}),
            )
            .unwrap();
            store::insert_run_job(
                &conn,
                &store::RunJobRecord {
                    id: id.into(),
                    run_group_id: group_id.into(),
                    benchmark_pack_id: "quick-smoke".into(),
                    status: status.into(),
                    message: String::new(),
                    started_at: "2026-01-01T00:00:00Z".into(),
                    finished_at: None,
                    total: 0,
                    completed: 0,
                    error: None,
                    request: serde_json::json!({}),
                    result_run_ids: vec![],
                },
            )
            .unwrap();
        }

        assert_eq!(clear_finished_jobs(&conn).unwrap(), 3);
        let jobs = list_jobs(&conn).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "running");
    }
}
