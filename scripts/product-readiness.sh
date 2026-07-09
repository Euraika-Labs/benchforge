#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAKE_BIN="${MAKE:-make}"
LOG_ROOT="${BENCHFORGE_PRODUCT_READINESS_LOG_DIR:-$ROOT/.benchforge/readiness}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$LOG_ROOT/product-$STAMP-$$"
STRICT="${BENCHFORGE_PRODUCT_READINESS_STRICT:-0}"
RUN_LOCAL="${BENCHFORGE_PRODUCT_READINESS_RUN_LOCAL:-0}"
RUN_FULL="${BENCHFORGE_PRODUCT_READINESS_RUN_FULL:-0}"
RUN_LIVE="${BENCHFORGE_PRODUCT_READINESS_RUN_LIVE:-0}"
RUN_DISTRIBUTION="${BENCHFORGE_PRODUCT_READINESS_RUN_DISTRIBUTION:-0}"

mkdir -p "$RUN_DIR"

WARNINGS=0
FAILURES=0

record() {
  printf "%s\n" "$*"
}

pass() {
  record "ok   $*"
}

warn() {
  WARNINGS=$((WARNINGS + 1))
  record "warn $*"
}

fail() {
  FAILURES=$((FAILURES + 1))
  record "fail $*"
}

run_make_target() {
  local target="$1"
  local log="$RUN_DIR/${target}.log"
  if "$MAKE_BIN" -C "$ROOT" "$target" >"$log" 2>&1; then
    pass "$target passed (log: $log)"
    return 0
  fi
  fail "$target failed (log: $log)"
  tail -40 "$log" | sed 's/^/     /'
  return 1
}

latest_readiness_summary() {
  if compgen -G "$LOG_ROOT"/*/summary.txt >/dev/null; then
    ls -t "$LOG_ROOT"/*/summary.txt | head -n 1
  fi
}

check_git_state() {
  if ! command -v git >/dev/null 2>&1 || ! git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    warn "git state not checked; this directory is not a Git worktree"
    return
  fi
  local status
  status="$(git -C "$ROOT" status --short)"
  if [[ -z "$status" ]]; then
    pass "Git worktree is clean"
  else
    warn "Git worktree has uncommitted changes; commit or stash before public handoff"
  fi
}

check_latest_readiness() {
  if [[ "$RUN_FULL" == "1" ]]; then
    run_make_target benchmark-readiness-full || true
    return
  fi
  if [[ "$RUN_LOCAL" == "1" ]]; then
    run_make_target benchmark-readiness || true
    return
  fi

  local summary
  summary="$(latest_readiness_summary || true)"
  if [[ -z "$summary" ]]; then
    warn "no benchmark-readiness summary found; run make benchmark-readiness-full before release handoff"
    return
  fi
  local mode
  mode="$(awk -F': ' '/^Mode:/ {print $2; exit}' "$summary")"
  if grep -q '^Benchmark-readiness passed\.$' "$summary"; then
    if [[ "$mode" == "full" ]]; then
      pass "latest full benchmark-readiness passed ($summary)"
    else
      warn "latest benchmark-readiness passed in $mode mode; run make benchmark-readiness-full before packaging"
    fi
  else
    fail "latest benchmark-readiness summary is not passing ($summary)"
  fi
}

provider_env_name() {
  case "$1" in
    openai) echo "OPENAI_API_KEY" ;;
    anthropic) echo "ANTHROPIC_API_KEY" ;;
    mistral) echo "MISTRAL_API_KEY" ;;
    openrouter) echo "OPENROUTER_API_KEY" ;;
    azure-openai) echo "AZURE_OPENAI_API_KEY" ;;
    gemini) echo "GEMINI_API_KEY" ;;
    *) echo "" ;;
  esac
}

provider_keychain_available() {
  local provider="$1"
  if ! command -v security >/dev/null 2>&1; then
    return 1
  fi
  security find-generic-password -a api_key -s "benchforge/cloud/$provider" >/dev/null 2>&1
}

configured_provider_key_summary() {
  local count=0
  local configured=()
  local provider env_name sources
  for provider in openai anthropic mistral openrouter azure-openai gemini; do
    sources=()
    env_name="$(provider_env_name "$provider")"
    if [[ -n "$env_name" && -n "${!env_name:-}" ]]; then
      sources+=("env")
    fi
    if provider_keychain_available "$provider"; then
      sources+=("keychain")
    fi
    if [[ "${#sources[@]}" -gt 0 ]]; then
      count=$((count + 1))
      configured+=("$provider($(IFS=+; echo "${sources[*]}"))")
    fi
  done
  printf "%s\n" "$count"
  if [[ "${#configured[@]}" -gt 0 ]]; then
    local detail=""
    local item
    for item in "${configured[@]}"; do
      if [[ -n "$detail" ]]; then
        detail+=", "
      fi
      detail+="$item"
    done
    printf "%s\n" "$detail"
  else
    printf "\n"
  fi
}

live_cloud_status_from_log() {
  local log="$1"
  python3 - "$log" <<'PY'
import json
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace")
decoder = json.JSONDecoder()
candidates = [0] if text.startswith("{") else []
candidates.extend(index + 1 for index, char in enumerate(text[:-1]) if char == "\n" and text[index + 1] == "{")
for start in reversed(candidates):
    try:
        payload, end = decoder.raw_decode(text[start:])
    except Exception:
        continue
    if text[start + end :].strip():
        continue
    print(payload.get("status", "unknown"))
    break
else:
    print("unknown")
PY
}

check_live_cloud() {
  if [[ "$RUN_LIVE" != "1" ]]; then
    local key_summary key_count key_detail
    key_summary="$(configured_provider_key_summary)"
    key_count="$(printf "%s\n" "$key_summary" | sed -n '1p')"
    key_detail="$(printf "%s\n" "$key_summary" | sed -n '2p')"
    if [[ "$key_count" -gt 0 ]]; then
      warn "$key_count live provider(s) with key material detected: $key_detail; run BENCHFORGE_PRODUCT_READINESS_RUN_LIVE=1 make product-readiness to validate them"
    else
      warn "live cloud providers not verified; run make live-cloud-smoke for validation or make live-cloud-run-basics for a paid live comparison"
    fi
    return
  fi

  local log="$RUN_DIR/live-cloud-smoke.log"
  if "$MAKE_BIN" -C "$ROOT" live-cloud-smoke >"$log" 2>&1; then
    local status
    status="$(live_cloud_status_from_log "$log")"
    case "$status" in
      completed)
        pass "live cloud benchmark completed (log: $log)"
        ;;
      validated)
        warn "live cloud providers validated but no paid benchmark ran; set BENCHFORGE_LIVE_CLOUD_RUN=1 for live comparison (log: $log)"
        ;;
      skipped)
        warn "live cloud verification skipped; no supported provider key was available (log: $log)"
        ;;
      partial)
        warn "live cloud verification completed partially; inspect setup/actions in $log"
        ;;
      *)
        warn "live cloud smoke returned unrecognized status '$status' (log: $log)"
        ;;
    esac
  else
    fail "live-cloud-smoke failed (log: $log)"
    tail -40 "$log" | sed 's/^/     /'
  fi
}

check_distribution_signing() {
  if [[ "$RUN_DISTRIBUTION" == "1" ]]; then
    if BENCHFORGE_RELEASE_DISTRIBUTION=1 "$MAKE_BIN" -C "$ROOT" release-signing-preflight >"$RUN_DIR/release-signing-preflight.log" 2>&1; then
      pass "distribution signing/notarization preflight passed"
    else
      fail "distribution signing/notarization preflight failed (log: $RUN_DIR/release-signing-preflight.log)"
      tail -60 "$RUN_DIR/release-signing-preflight.log" | sed 's/^/     /'
    fi
    return
  fi

  warn "public distribution signing not verified; run make release-signing-plan, then BENCHFORGE_PRODUCT_READINESS_RUN_DISTRIBUTION=1 make product-readiness"
}

record "BenchForge product-readiness summary"
record "Logs: $RUN_DIR"
record "Safe default: no live provider calls, paid benchmarks, or Apple signing checks are run unless explicitly requested."
record ""

check_git_state
run_make_target release-preflight || true
check_latest_readiness
check_live_cloud
check_distribution_signing

record ""
record "Summary: $FAILURES failure(s), $WARNINGS warning(s)"
if [[ "$FAILURES" -gt 0 ]]; then
  exit 1
fi
if [[ "$STRICT" == "1" && "$WARNINGS" -gt 0 ]]; then
  record "Strict mode treats warnings as blockers."
  exit 1
fi
exit 0
