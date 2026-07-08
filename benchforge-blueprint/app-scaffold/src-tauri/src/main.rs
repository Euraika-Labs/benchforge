mod adapters;
mod commands;
mod diagnostics;
mod harness_tools;
mod huggingface;
mod jobs;
mod metrics;
mod paths;
mod runner;
mod runtime_tools;
mod safety;
mod sandbox;
mod secrets;
mod store;
mod targeting;

fn main() {
    diagnostics::install_panic_hook();

    if std::env::args().any(|arg| arg == "--benchforge-smoke") {
        let docker = std::env::args().any(|arg| arg == "--docker");
        if let Err(err) = runner::run_cli_smoke(docker) {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-prompt-smoke") {
        if let Err(err) = runner::run_cli_prompt_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-llm-connectivity-smoke") {
        if let Err(err) = runner::run_cli_llm_connectivity_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-llm-core-smoke") {
        if let Err(err) = runner::run_cli_llm_core_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-llm-practical-smoke") {
        if let Err(err) = runner::run_cli_llm_practical_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-llm-decision-smoke") {
        if let Err(err) = runner::run_cli_llm_decision_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-llm-structured-smoke") {
        if let Err(err) = runner::run_cli_llm_structured_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-llm-grounded-smoke") {
        if let Err(err) = runner::run_cli_llm_grounded_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-llm-reliability-smoke") {
        if let Err(err) = runner::run_cli_llm_reliability_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-code-edit-smoke") {
        if let Err(err) = runner::run_cli_code_edit_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-code-edit-contract-smoke") {
        if let Err(err) = runner::run_cli_code_edit_contract_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-security-smoke") {
        if let Err(err) = runner::run_cli_security_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-worker-harness-contract-smoke") {
        if let Err(err) = runner::run_cli_worker_harness_contract_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-cloud-contract-smoke") {
        if let Err(err) = runner::run_cli_cloud_contract_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-provider-error-contract-smoke") {
        if let Err(err) = runner::run_cli_provider_error_contract_smoke() {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-validation-contract-smoke") {
        match commands::run_cli_validation_contract_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-create-target-handoff-smoke") {
        match commands::run_cli_create_target_handoff_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-connectivity-smoke") {
        match commands::run_cli_local_cloud_connectivity_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-compare-smoke") {
        match commands::run_cli_local_cloud_compare_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-job-smoke") {
        match commands::run_cli_local_cloud_job_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-basics-smoke") {
        match commands::run_cli_local_cloud_basics_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-core-smoke") {
        match commands::run_cli_local_cloud_core_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-practical-smoke") {
        match commands::run_cli_local_cloud_practical_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-decision-smoke") {
        match commands::run_cli_local_cloud_decision_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-structured-smoke") {
        match commands::run_cli_local_cloud_structured_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-grounded-smoke") {
        match commands::run_cli_local_cloud_grounded_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-cloud-reliability-smoke") {
        match commands::run_cli_local_cloud_reliability_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-cloud-provider-job-smoke") {
        match commands::run_cli_cloud_provider_job_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-cloud-catalog-smoke") {
        match commands::run_cli_cloud_catalog_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-local-runtime-discovery-smoke") {
        match commands::run_cli_local_runtime_discovery_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-live-cloud-smoke") {
        match commands::run_cli_live_cloud_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-worker-mock") {
        match store::open_app() {
            Ok(conn) => match runner::run_worker_mock(&conn) {
                Ok(result) => println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                ),
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            },
            Err(err) => {
                eprintln!("failed to open store: {err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-job-smoke") {
        match store::open_app() {
            Ok(conn) => match run_cli_job_smoke(&conn) {
                Ok(job) => {
                    println!("{}", serde_json::to_string_pretty(&job).unwrap_or_default());
                    return;
                }
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            },
            Err(err) => {
                eprintln!("failed to open store: {err}");
                std::process::exit(1);
            }
        }
    }
    if std::env::args().any(|arg| arg == "--benchforge-report-smoke") {
        match store::open_app() {
            Ok(conn) => match run_cli_report_smoke(&conn) {
                Ok(summary) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&summary).unwrap_or_default()
                    );
                    return;
                }
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            },
            Err(err) => {
                eprintln!("failed to open store: {err}");
                std::process::exit(1);
            }
        }
    }
    if std::env::args().any(|arg| arg == "--benchforge-first-run-smoke") {
        match commands::run_cli_first_run_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-path-smoke") {
        let summary = serde_json::json!({
            "status": "ok",
            "dataDir": paths::app_data_dir().to_string_lossy(),
            "dataDirOverride": std::env::var("BENCHFORGE_DATA_DIR").ok(),
            "resourceDir": paths::resource_root().to_string_lossy(),
            "resourceDirOverride": std::env::var("BENCHFORGE_RESOURCE_DIR").ok(),
            "workerLauncher": paths::bundled_worker_launcher().to_string_lossy(),
            "workerPackageDir": paths::bundled_worker_package_dir().to_string_lossy(),
            "dbPath": paths::db_path().to_string_lossy(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-search-smoke") {
        match commands::run_cli_hf_search_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-download-smoke") {
        match huggingface::download_model(huggingface::DownloadModelRequest {
            repo_id: "ggml-org/tinygemma3-GGUF".into(),
            filename: Some("tinygemma3-Q8_0.gguf".into()),
            revision: None,
            download_id: None,
            start_after_download: false,
            run_connectivity_after_start: false,
            auto_benchmark_pack_id: None,
            auto_compare_after_start: false,
            start_port: None,
            start_context: None,
        }) {
            Ok(result) => println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-download-job-smoke") {
        match run_cli_hf_download_job_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-download-start-job-smoke") {
        match commands::run_cli_hf_download_start_job_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-server-job-smoke") {
        match run_cli_hf_server_job_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-server-start-job-smoke") {
        match commands::run_cli_hf_server_start_job_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-local-smoke") {
        match run_cli_hf_local_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-local-cloud-smoke") {
        match commands::run_cli_hf_local_cloud_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--benchforge-hf-local-cloud-basics-smoke") {
        match commands::run_cli_hf_local_cloud_basics_smoke() {
            Ok(summary) => println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            ),
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        return;
    }

    tauri::Builder::default()
        .manage(store::init_state().expect("failed to initialize BenchForge store"))
        .invoke_handler(tauri::generate_handler![
            commands::get_app_version,
            commands::list_adapters,
            commands::list_targets,
            commands::create_target,
            commands::create_target_with_benchmark_handoff,
            commands::set_target_enabled,
            commands::delete_target,
            commands::export_target_redacted,
            commands::validate_target,
            commands::save_provider_api_key,
            commands::provider_api_key_status,
            commands::list_benchmark_packs,
            commands::list_benchmark_pack_diagnostics,
            commands::list_benchmark_pack_tasks,
            commands::create_benchmark_pack_template,
            commands::add_benchmark_pack_prompt_task,
            commands::update_benchmark_pack_prompt_task,
            commands::update_benchmark_pack_calibration,
            commands::suggest_benchmark_pack_calibration,
            commands::score_prompt_task_preview,
            commands::delete_benchmark_pack_task,
            commands::export_benchmark_pack,
            commands::import_benchmark_pack,
            commands::detect_local_runtimes,
            commands::run_local_runtime_tool_action,
            commands::search_cloud_models,
            commands::estimate_run_plan,
            commands::run_quick_smoke,
            commands::start_run_job,
            commands::list_run_jobs,
            commands::get_run_job,
            commands::cancel_run_job,
            commands::duplicate_run_job,
            commands::retry_run_job,
            commands::clear_finished_run_jobs,
            commands::run_worker_mock,
            commands::list_results,
            commands::list_artifacts,
            commands::read_artifact,
            commands::export_results,
            commands::export_report_folder,
            commands::huggingface_status,
            commands::save_huggingface_token,
            commands::install_huggingface_tools,
            commands::run_harness_tool_action,
            commands::search_huggingface_models,
            commands::inspect_huggingface_model,
            commands::plan_huggingface_download,
            commands::download_huggingface_model,
            commands::start_huggingface_download_job,
            commands::list_huggingface_download_jobs,
            commands::get_huggingface_download_job,
            commands::cancel_huggingface_download_job,
            commands::retry_huggingface_download_job,
            commands::clear_finished_huggingface_download_jobs,
            commands::reveal_huggingface_model,
            commands::delete_huggingface_model,
            commands::preflight_huggingface_model,
            commands::start_huggingface_model,
            commands::start_huggingface_server_job,
            commands::list_huggingface_server_jobs,
            commands::get_huggingface_server_job,
            commands::cancel_huggingface_server_job,
            commands::retry_huggingface_server_job,
            commands::clear_finished_huggingface_server_jobs,
            commands::stop_huggingface_model,
            commands::record_diagnostic_event,
            commands::list_diagnostics,
            commands::run_doctor
        ])
        .run(tauri::generate_context!())
        .expect("error while running BenchForge");
}

fn reserve_loopback_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|err| format!("failed to reserve a local model smoke port: {}", err))?;
    let port = listener
        .local_addr()
        .map_err(|err| format!("failed to read reserved local port: {}", err))?
        .port();
    drop(listener);
    Ok(port)
}

fn run_cli_hf_local_smoke() -> Result<serde_json::Value, String> {
    let state = store::init_state().map_err(|err| format!("failed to initialize store: {err}"))?;
    let port = reserve_loopback_port()?;
    let repo_id = "ggml-org/tinygemma3-GGUF".to_string();
    let filename = "tinygemma3-Q8_0.gguf".to_string();
    let context = 512_u32;
    let download = huggingface::download_model(huggingface::DownloadModelRequest {
        repo_id: repo_id.clone(),
        filename: Some(filename.clone()),
        revision: None,
        download_id: None,
        start_after_download: false,
        run_connectivity_after_start: false,
        auto_benchmark_pack_id: None,
        auto_compare_after_start: false,
        start_port: None,
        start_context: None,
    })?;

    let start_status = match huggingface::start_server(
        &state,
        huggingface::StartModelRequest {
            repo_id: repo_id.clone(),
            filename: Some(filename.clone()),
            port,
            context,
            register_target_after_start: false,
            run_connectivity_after_start: false,
            auto_benchmark_pack_id: None,
            auto_compare_after_start: false,
        },
    ) {
        Ok(status) => status,
        Err(err) => {
            let _ = huggingface::stop_server(&state);
            return Err(err);
        }
    };

    let target_id = "hf-local-smoke".to_string();
    let served_model = start_status
        .server_model_id
        .clone()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| filename.clone());
    let target = store::NewTarget {
        id: target_id.clone(),
        name: format!("HF Local Smoke {}", filename.trim_end_matches(".gguf")),
        kind: "direct_model".into(),
        adapter_id: "llama-cpp-openai".into(),
        config: serde_json::json!({
            "model": served_model,
            "base_url": format!("http://127.0.0.1:{}/v1", port),
            "source": "huggingface-local-smoke",
            "repo_id": repo_id,
            "gguf_file": filename,
            "model_path": download.path.clone(),
            "port": port,
            "context": context,
            "temperature": 0,
            "top_p": 1,
            "max_tokens": 512,
            "timeout_seconds": 120,
            "retry_count": 1,
            "input_price_usd_per_million_tokens": 0,
            "output_price_usd_per_million_tokens": 0
        }),
    };

    let run_attempt: Result<Vec<runner::RunResultDto>, String> = {
        let conn = state
            .conn
            .lock()
            .map_err(|err| format!("failed to lock store: {err}"))?;
        store::upsert_target(&conn, &target).map_err(|err| err.to_string())?;
        let pack = runner::load_pack("llm-connectivity")?;
        let expected_tasks = runner::load_tasks(&pack)?.len();
        let results = runner::run_quick_smoke(
            &conn,
            runner::RunQuickSmokeRequest {
                target_ids: vec![target_id.clone()],
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
        if results.len() != expected_tasks {
            Err(format!(
                "hf_local_smoke_failed: expected {} llm-connectivity result(s), got {}",
                expected_tasks,
                results.len()
            ))
        } else if results.iter().any(|result| result.status == "error") {
            Err(format!(
                "hf_local_smoke_failed: runner returned error result(s): {}",
                serde_json::to_string(&results).unwrap_or_else(|_| "results unavailable".into())
            ))
        } else {
            Ok(results)
        }
    };

    let stop_status = huggingface::stop_server(&state)?;
    let run_results = run_attempt?;
    let passed = run_results
        .iter()
        .filter(|result| result.status == "passed")
        .count();

    Ok(serde_json::json!({
        "download": download,
        "port": port,
        "targetId": target_id,
        "benchmarkPackId": "llm-connectivity",
        "resultCount": run_results.len(),
        "passedResults": passed,
        "serverStopped": !stop_status.server_running,
        "results": run_results
    }))
}

fn run_cli_hf_download_job_smoke() -> Result<serde_json::Value, String> {
    let conn = store::open_app().map_err(|err| format!("failed to open store: {err}"))?;
    let job = huggingface::start_download_job(
        &conn,
        huggingface::DownloadModelRequest {
            repo_id: "ggml-org/tinygemma3-GGUF".into(),
            filename: Some("tinygemma3-Q8_0.gguf".into()),
            revision: None,
            download_id: None,
            start_after_download: false,
            run_connectivity_after_start: false,
            auto_benchmark_pack_id: None,
            auto_compare_after_start: false,
            start_port: None,
            start_context: None,
        },
    )?;
    let finished = wait_for_cli_hf_download_job(&conn, &job.id)?;
    if finished.status != "completed" {
        return Err(format!(
            "hf_download_job_smoke_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    let model = finished.model.as_ref().ok_or_else(|| {
        "hf_download_job_smoke_failed: completed job has no model metadata".to_string()
    })?;
    if model.selected_file.as_deref() != Some("tinygemma3-Q8_0.gguf") {
        return Err(format!(
            "hf_download_job_smoke_failed: unexpected selected file {:?}",
            model.selected_file
        ));
    }
    Ok(serde_json::json!({
        "job": finished,
        "modelPath": model.path,
        "selectedFile": model.selected_file
    }))
}

fn run_cli_hf_server_job_smoke() -> Result<serde_json::Value, String> {
    let state = store::init_state().map_err(|err| format!("failed to initialize store: {err}"))?;
    let port = reserve_loopback_port()?;
    let repo_id = "ggml-org/tinygemma3-GGUF".to_string();
    let filename = "tinygemma3-Q8_0.gguf".to_string();
    let context = 512_u32;
    let download = huggingface::download_model(huggingface::DownloadModelRequest {
        repo_id: repo_id.clone(),
        filename: Some(filename.clone()),
        revision: None,
        download_id: None,
        start_after_download: false,
        run_connectivity_after_start: false,
        auto_benchmark_pack_id: None,
        auto_compare_after_start: false,
        start_port: None,
        start_context: None,
    })?;
    let request = huggingface::normalize_start_request(huggingface::StartModelRequest {
        repo_id: repo_id.clone(),
        filename: Some(filename.clone()),
        port,
        context,
        register_target_after_start: false,
        run_connectivity_after_start: false,
        auto_benchmark_pack_id: None,
        auto_compare_after_start: false,
    })?;
    let job = {
        let conn = state
            .conn
            .lock()
            .map_err(|err| format!("failed to lock store: {err}"))?;
        huggingface::enqueue_server_job(&conn, request.clone())?
    };

    huggingface::run_server_job(&state, job.id.clone(), request);
    let finished = {
        let conn = state
            .conn
            .lock()
            .map_err(|err| format!("failed to lock store: {err}"))?;
        huggingface::get_server_job(&conn, &job.id)?
            .ok_or_else(|| format!("hf server job {} disappeared", job.id))?
    };
    let stop_status = huggingface::stop_server(&state).ok();
    if finished.status != "completed" {
        return Err(format!(
            "hf_server_job_smoke_failed: job {} finished with status {} error {:?}",
            finished.id, finished.status, finished.error
        ));
    }
    let server_status = finished.server_status.as_ref().ok_or_else(|| {
        "hf_server_job_smoke_failed: completed job has no server status".to_string()
    })?;
    if !server_status.server_running {
        return Err(
            "hf_server_job_smoke_failed: stored status did not report a running server".into(),
        );
    }
    Ok(serde_json::json!({
        "job": finished,
        "download": download,
        "port": port,
        "serverStopped": stop_status.map(|status| !status.server_running).unwrap_or(false)
    }))
}

fn run_cli_job_smoke(conn: &rusqlite::Connection) -> Result<jobs::RunJobDto, String> {
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
    let job = jobs::start_quick_smoke_job(conn, request)?;
    let finished = wait_for_cli_job(conn, &job.id)?;
    if finished.status != "completed" {
        return Err(format!(
            "job smoke failed: job {} finished with status {}",
            finished.id, finished.status
        ));
    }
    Ok(finished)
}

fn wait_for_cli_hf_download_job(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<huggingface::HuggingFaceDownloadJobDto, String> {
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > std::time::Duration::from_secs(60) {
            return Err(format!(
                "hf download job smoke timed out waiting for job {}",
                id
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        match huggingface::get_download_job(conn, id)? {
            Some(job)
                if job.status == "queued"
                    || job.status == "running"
                    || job.status == "cancelling" =>
            {
                continue
            }
            Some(job) => return Ok(job),
            None => return Err(format!("hf download job {} disappeared", id)),
        }
    }
}

fn wait_for_cli_job(conn: &rusqlite::Connection, id: &str) -> Result<jobs::RunJobDto, String> {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        match jobs::get_job(conn, id)? {
            Some(next) if next.status == "queued" || next.status == "running" => continue,
            Some(next) => return Ok(next),
            None => return Err(format!("job disappeared: {}", id)),
        }
    }
}

fn run_cli_report_smoke(conn: &rusqlite::Connection) -> Result<serde_json::Value, String> {
    let job = run_cli_job_smoke(conn)?;
    let run_ids = job
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let export_path = commands::export_report_folder_for_conn(conn, Some(run_ids))?;
    let readme_path = std::path::Path::new(&export_path).join("README.md");
    let readme = std::fs::read_to_string(&readme_path).map_err(|err| err.to_string())?;
    if !readme.contains("## Run Configuration") {
        return Err("report smoke failed: README has no run configuration section".into());
    }
    if !readme.contains("## Metric Coverage") {
        return Err("report smoke failed: README has no metric coverage section".into());
    }
    if !readme.contains("max 4096") {
        return Err(
            "report smoke failed: README does not summarize queued generation settings".into(),
        );
    }
    let reproducibility_path = std::path::Path::new(&export_path).join("reproducibility.json");
    let reproducibility_raw =
        std::fs::read_to_string(&reproducibility_path).map_err(|err| err.to_string())?;
    let reproducibility: serde_json::Value =
        serde_json::from_str(&reproducibility_raw).map_err(|err| err.to_string())?;
    let group = reproducibility
        .get("run_groups")
        .and_then(|value| value.as_array())
        .and_then(|groups| groups.first())
        .ok_or_else(|| {
            "report smoke failed: reproducibility manifest has no run group".to_string()
        })?;
    let queued = group
        .get("queued_run_group")
        .ok_or_else(|| "report smoke failed: missing queued run group snapshot".to_string())?;
    if queued.get("config").is_none() {
        return Err("report smoke failed: queued run group has no config snapshot".into());
    }
    if queued
        .pointer("/config/targets/0/generation/max_tokens")
        .is_none()
    {
        return Err(
            "report smoke failed: queued run group target generation snapshot is missing".into(),
        );
    }
    Ok(serde_json::json!({
        "job": job,
        "exportPath": export_path,
        "queuedRunGroup": queued
    }))
}
