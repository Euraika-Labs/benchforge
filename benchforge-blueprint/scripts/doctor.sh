#!/usr/bin/env bash
set -uo pipefail

FAILED=0

first_line() {
  "$@" 2>&1 | awk 'NR == 1 { print; exit }' || true
}

check_required() {
  local name="$1"; shift
  if command -v "$name" >/dev/null 2>&1; then
    echo "ok   $name: $(first_line "$name" "$@")"
  else
    echo "fail $name: not found"
    FAILED=1
  fi
}

check_optional() {
  local name="$1"; shift
  if command -v "$name" >/dev/null 2>&1; then
    echo "ok   $name: $(first_line "$name" "$@")"
  else
    echo "warn $name: not found"
  fi
}

python_meets_minimum() {
  local python_bin="$1"
  "$python_bin" - <<'PY' >/dev/null 2>&1
import sys
raise SystemExit(0 if sys.version_info >= (3, 10) else 1)
PY
}

resolve_python_candidate() {
  local candidate="$1"
  if command -v "$candidate" >/dev/null 2>&1; then
    command -v "$candidate"
    return 0
  fi
  if [[ -x "$candidate" ]]; then
    printf "%s\n" "$candidate"
    return 0
  fi
  return 1
}

check_python() {
  local candidates=()
  local candidate python_bin version_line old_versions=()

  if [[ -n "${BENCHFORGE_PYTHON:-}" ]]; then
    candidates=("$BENCHFORGE_PYTHON")
  else
    candidates=(python3 python3.13 python3.12 python3.11 python3.10 /opt/homebrew/bin/python3 /usr/local/bin/python3)
  fi

  for candidate in "${candidates[@]}"; do
    python_bin="$(resolve_python_candidate "$candidate" || true)"
    [[ -n "$python_bin" ]] || continue

    version_line="$("$python_bin" --version 2>&1 | head -n 1)"
    if python_meets_minimum "$python_bin"; then
      echo "ok   python: $version_line ($python_bin)"
      return 0
    fi
    old_versions+=("$version_line at $python_bin")
  done

  if [[ "${#old_versions[@]}" -gt 0 ]]; then
    echo "fail python: Python 3.10+ required; found ${old_versions[*]}"
  else
    echo "fail python: Python 3.10+ not found"
  fi
  echo "hint python: install with 'brew install python' or set BENCHFORGE_PYTHON=/path/to/python3.10+"
  FAILED=1
}

check_required git --version
check_required node --version
check_required npm --version
check_required cargo --version
check_python

check_optional docker --version
check_optional colima version
check_optional codex --version
check_optional claude --version
check_optional vibe --version
check_optional copilot help
check_optional hf version
check_optional llama-server --version

exit "$FAILED"
