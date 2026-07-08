#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PYTHON_CANDIDATES=()

add_python_candidate() {
  local candidate="${1:-}"
  local existing
  [[ -n "$candidate" ]] || return 0
  if [[ "${#PYTHON_CANDIDATES[@]}" -gt 0 ]]; then
    for existing in "${PYTHON_CANDIDATES[@]}"; do
      [[ "$existing" == "$candidate" ]] && return 0
    done
  fi
  PYTHON_CANDIDATES+=("$candidate")
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

select_python() {
  local candidate python_bin version_line old_versions=()

  if [[ -n "${BENCHFORGE_PYTHON:-}" ]]; then
    add_python_candidate "$BENCHFORGE_PYTHON"
  else
    add_python_candidate python3
    add_python_candidate python3.13
    add_python_candidate python3.12
    add_python_candidate python3.11
    add_python_candidate python3.10
    add_python_candidate /opt/homebrew/bin/python3
    add_python_candidate /usr/local/bin/python3
  fi

  for candidate in "${PYTHON_CANDIDATES[@]}"; do
    python_bin="$(resolve_python_candidate "$candidate" || true)"
    [[ -n "$python_bin" ]] || continue

    version_line="$("$python_bin" --version 2>&1 | head -n 1)"
    if python_meets_minimum "$python_bin"; then
      printf "%s\n" "$python_bin"
      return 0
    fi
    old_versions+=("$version_line at $python_bin")
  done

  {
    echo "BenchForge requires Python 3.10+ for the worker package and Hugging Face CLI."
    if [[ "${#old_versions[@]}" -gt 0 ]]; then
      echo "Found older Python interpreter(s):"
      printf "  - %s\n" "${old_versions[@]}"
    else
      echo "No usable Python interpreter was found."
    fi
    echo "Install a current Python with: brew install python"
    echo "Or rerun with BENCHFORGE_PYTHON=/path/to/python3.10+ ./scripts/bootstrap.sh"
  } >&2
  return 1
}

PYTHON_BIN="$(select_python)"
echo "Using Python: $("$PYTHON_BIN" --version 2>&1) ($PYTHON_BIN)"

cd "$ROOT/app-scaffold"
npm install
cd "$ROOT/workers"
"$PYTHON_BIN" -m venv .venv
source .venv/bin/activate
python -m pip install -e .
