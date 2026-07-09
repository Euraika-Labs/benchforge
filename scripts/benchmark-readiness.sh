#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAKE_BIN="${MAKE:-make}"
MODE="${1:-${BENCHFORGE_READINESS_MODE:-quick}}"
CONTINUE_ON_FAILURE="${BENCHFORGE_READINESS_CONTINUE:-0}"
LOG_ROOT="${BENCHFORGE_READINESS_LOG_DIR:-$ROOT/.benchforge/readiness}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$LOG_ROOT/$STAMP-$$"
SUMMARY="$RUN_DIR/summary.txt"
SKIP_PROCESS_POSTFLIGHT="${BENCHFORGE_READINESS_SKIP_PROCESS_POSTFLIGHT:-0}"
TARGET_TIMEOUT_SECONDS="${BENCHFORGE_READINESS_TARGET_TIMEOUT_SECONDS:-}"

case "$MODE" in
  quick|full) ;;
  *)
    echo "Usage: $0 [quick|full]"
    echo "Set BENCHFORGE_READINESS_CONTINUE=1 to run remaining checks after a failure."
    exit 2
    ;;
esac

if [[ -z "$TARGET_TIMEOUT_SECONDS" ]]; then
  if [[ "$MODE" == "full" ]]; then
    TARGET_TIMEOUT_SECONDS=1800
  else
    TARGET_TIMEOUT_SECONDS=900
  fi
fi

if ! [[ "$TARGET_TIMEOUT_SECONDS" =~ ^[0-9]+$ ]]; then
  echo "BENCHFORGE_READINESS_TARGET_TIMEOUT_SECONDS must be a non-negative integer"
  exit 2
fi

mkdir -p "$RUN_DIR"
: > "$SUMMARY"

record() {
  printf "%s\n" "$*" | tee -a "$SUMMARY"
}

collect_descendants() {
  local pid="$1"
  local child

  for child in $(pgrep -P "$pid" 2>/dev/null || true); do
    collect_descendants "$child"
    printf "%s\n" "$child"
  done
}

make_target_is_running() {
  local pid="$1"
  jobs -pr | grep -qx "$pid"
}

terminate_process_tree() {
  local pid="$1"
  local descendants

  descendants="$(collect_descendants "$pid" || true)"
  for child in $descendants; do
    kill -TERM "$child" 2>/dev/null || true
  done
  kill -TERM "$pid" 2>/dev/null || true

  for _ in 1 2 3 4 5; do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done

  for child in $descendants; do
    kill -KILL "$child" 2>/dev/null || true
  done
  kill -KILL "$pid" 2>/dev/null || true
}

run_make_target() {
  local target="$1"
  local description="$2"
  local log="$RUN_DIR/${target}.log"
  local pid
  local started_at
  local elapsed
  local rc

  record ""
  record "==> $target"
  record "$description"
  "$MAKE_BIN" -C "$ROOT" "$target" >"$log" 2>&1 &
  pid=$!
  started_at=$SECONDS

  while make_target_is_running "$pid"; do
    elapsed=$((SECONDS - started_at))
    if [[ "$TARGET_TIMEOUT_SECONDS" -gt 0 && "$elapsed" -ge "$TARGET_TIMEOUT_SECONDS" ]]; then
      record "fail $target timed out after ${TARGET_TIMEOUT_SECONDS}s (log: $log)"
      terminate_process_tree "$pid"
      wait "$pid" 2>/dev/null || true
      record "--- tail: $target ---"
      tail -80 "$log" | tee -a "$SUMMARY"
      record "--- end tail ---"
      return 124
    fi
    sleep 1
  done

  if wait "$pid"; then
    record "ok   $target (log: $log)"
    return 0
  else
    rc=$?
    record "fail $target exited with $rc (log: $log)"
    record "--- tail: $target ---"
    tail -80 "$log" | tee -a "$SUMMARY"
    record "--- end tail ---"
    return "$rc"
  fi
}

run_local_server_postflight() {
  local log="$RUN_DIR/local-server-postflight.log"

  record ""
  record "==> local-server-postflight"
  record "Checks that readiness did not leave a llama-server process serving from this repo's .benchforge/models cache."

  if [[ "$SKIP_PROCESS_POSTFLIGHT" == "1" ]]; then
    : > "$log"
    record "skip local-server-postflight (BENCHFORGE_READINESS_SKIP_PROCESS_POSTFLIGHT=1)"
    return 0
  fi

  local leaked
  leaked="$(ps -axo pid=,command= | grep "[l]lama-server" | grep -F "$ROOT/.benchforge/models" || true)"
  if [[ -n "$leaked" ]]; then
    printf "%s\n" "$leaked" > "$log"
    record "fail local-server-postflight found BenchForge model-cache llama-server process(es) (log: $log)"
    record "--- leaked llama-server processes ---"
    printf "%s\n" "$leaked" | tee -a "$SUMMARY"
    record "--- end leaked llama-server processes ---"
    return 1
  fi

  : > "$log"
  record "ok   local-server-postflight (log: $log)"
  return 0
}

stop_after_failure_if_needed() {
  if [[ "$FAILED" -ne 0 && "$CONTINUE_ON_FAILURE" != "1" ]]; then
    run_local_server_postflight || true
    record "Stopping after first failure. Set BENCHFORGE_READINESS_CONTINUE=1 to continue."
    exit 1
  fi
}

record "BenchForge benchmark-readiness gate"
record "Mode: $MODE"
record "Logs: $RUN_DIR"
if [[ "$TARGET_TIMEOUT_SECONDS" -eq 0 ]]; then
  record "Per-target timeout: disabled"
else
  record "Per-target timeout: ${TARGET_TIMEOUT_SECONDS}s"
fi
record "Scope: clean first-run, local/cloud contracts, cloud model catalogs, local runtime discovery, validation, reports, worker harness imports, Hugging Face GGUF search/planning, and local-cloud workflow."
record "Note: this does not spend cloud API credits; provider checks use loopback contract servers."

FAILED=0

run_make_target doctor "Checks local development/runtime tools and gives remediation hints." || FAILED=1
stop_after_failure_if_needed

run_make_target test "Builds the web app, validates schemas, runs Rust tests, and runs worker tests." || FAILED=1
stop_after_failure_if_needed

run_make_target worker-harness-contract-smoke "Verifies external harness command execution and JSON/JSONL/CSV/JUnit result import contracts." || FAILED=1
stop_after_failure_if_needed

run_make_target first-run-smoke "Verifies clean app-data initialization, seeded target state, Doctor readiness, queued smoke run, and report export." || FAILED=1
stop_after_failure_if_needed

run_make_target validation-contract-smoke "Verifies target validation success and normalized provider setup errors." || FAILED=1
stop_after_failure_if_needed

run_make_target cloud-provider-job-smoke "Runs cloud adapter contracts through persisted jobs, metrics, snapshots, and report export." || FAILED=1
stop_after_failure_if_needed

run_make_target cloud-catalog-smoke "Verifies cloud model catalog presets, provider model-list parsing, pricing/context metadata, and catalog-to-target redaction." || FAILED=1
stop_after_failure_if_needed

run_make_target local-runtime-discovery-smoke "Verifies local runtime detection shapes, native Ollama tag fallback, target creation, validation, and connectivity runs for common local adapters." || FAILED=1
stop_after_failure_if_needed

run_make_target local-cloud-basics-smoke "Compares local-style and cloud-style OpenAI-compatible targets on llm-basics." || FAILED=1
stop_after_failure_if_needed

run_make_target hf-search-smoke "Verifies Hugging Face popular GGUF browsing, query search, file inspection, and download planning." || FAILED=1
stop_after_failure_if_needed

run_make_target hf-local-cloud-basics-smoke "Downloads/reuses a tiny public GGUF, starts llama-server, benchmarks beside cloud-style target, exports, and stops." || FAILED=1
stop_after_failure_if_needed

if [[ "$MODE" == "full" ]]; then
  run_make_target release-preflight "Verifies license, community files, dependency advisory scope, Tauri bundle metadata, package lock, icon, and release docs before packaging." || FAILED=1
  stop_after_failure_if_needed

  run_make_target provider-error-contract-smoke "Verifies normalized runtime provider errors and transport metrics." || FAILED=1
  stop_after_failure_if_needed

  run_make_target create-target-handoff-smoke "Verifies cloud target creation can validate and queue an automatic benchmark from the backend." || FAILED=1
  stop_after_failure_if_needed

  run_make_target local-cloud-reliability-smoke "Runs the strongest offline local/cloud quality-pack comparison gate." || FAILED=1
  stop_after_failure_if_needed

  run_make_target package-dmg "Builds the unsigned macOS DMG and prints the generated bundle path." || FAILED=1
  stop_after_failure_if_needed

  run_make_target install-smoke-dmg "Copies the app out of the DMG and verifies first-run benchmark execution from bundled resources." || FAILED=1
  stop_after_failure_if_needed
fi

run_local_server_postflight || FAILED=1

record ""
if [[ "$FAILED" -eq 0 ]]; then
  record "Benchmark-readiness passed."
  record "Summary: $SUMMARY"
  exit 0
fi

record "Benchmark-readiness failed. Inspect logs under $RUN_DIR."
exit 1
