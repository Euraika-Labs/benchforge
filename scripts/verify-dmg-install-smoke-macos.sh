#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DMG_PATH="${1:-}"
EXPECTED_NAME="${BENCHFORGE_EXPECTED_PRODUCT_NAME:-BenchForge}"
LOG_DIR="${BENCHFORGE_INSTALL_SMOKE_LOG_DIR:-$ROOT/.benchforge/install-smoke}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "DMG install smoke requires macOS." >&2
  exit 1
fi

if [[ -z "$DMG_PATH" ]]; then
  DMG_DIR="$ROOT/app-scaffold/src-tauri/target/release/bundle/dmg"
  if ! compgen -G "$DMG_DIR/*.dmg" >/dev/null; then
    echo "No DMG found in $DMG_DIR. Run make package-dmg first." >&2
    exit 1
  fi
  DMG_PATH="$(ls -t "$DMG_DIR"/*.dmg | head -n 1)"
fi

if [[ ! -f "$DMG_PATH" ]]; then
  echo "DMG not found: $DMG_PATH" >&2
  exit 1
fi

mkdir -p "$LOG_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
LOG_PATH="$LOG_DIR/install-smoke-$STAMP.json"
PATH_LOG_PATH="$LOG_DIR/install-path-smoke-$STAMP.json"
WORKER_LOG_PATH="$LOG_DIR/install-worker-smoke-$STAMP.json"
SECURITY_LOG_PATH="$LOG_DIR/install-security-smoke-$STAMP.json"
MOUNT_POINT="$(mktemp -d "${TMPDIR:-/tmp}/benchforge-dmg-smoke-mount.XXXXXX")"
INSTALL_DIR="$(mktemp -d "${TMPDIR:-/tmp}/benchforge-dmg-smoke-install.XXXXXX")"
DATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/benchforge-dmg-smoke-data.XXXXXX")"
HOME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/benchforge-dmg-smoke-home.XXXXXX")"
ATTACHED=0

cleanup() {
  if [[ "$ATTACHED" == "1" ]]; then
    diskutil eject "$MOUNT_POINT" >/dev/null 2>&1 || hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
  fi
  rm -rf "$MOUNT_POINT" "$INSTALL_DIR" "$DATA_DIR" "$HOME_DIR"
}
trap cleanup EXIT

require_path() {
  local path="$1"
  local description="$2"
  if [[ ! -e "$path" ]]; then
    echo "Missing $description: $path" >&2
    exit 1
  fi
}

hdiutil attach -quiet -readonly -nobrowse -noautoopen -mountpoint "$MOUNT_POINT" "$DMG_PATH"
ATTACHED=1

APP_SOURCE="$MOUNT_POINT/$EXPECTED_NAME.app"
require_path "$APP_SOURCE" "app bundle"

APP_PATH="$INSTALL_DIR/$EXPECTED_NAME.app"
ditto "$APP_SOURCE" "$APP_PATH"

diskutil eject "$MOUNT_POINT" >/dev/null
ATTACHED=0

INFO_PLIST="$APP_PATH/Contents/Info.plist"
RESOURCES_DIR="$APP_PATH/Contents/Resources"
require_path "$INFO_PLIST" "Info.plist"
require_path "$RESOURCES_DIR" "app resources directory"

for resource in adapters benchmark-packs docker fixtures workers/benchforge_worker; do
  require_path "$RESOURCES_DIR/$resource" "bundled $resource resource"
done
require_path "$RESOURCES_DIR/workers/benchforge-worker" "bundled worker launcher"
if [[ ! -x "$RESOURCES_DIR/workers/benchforge-worker" ]]; then
  echo "Bundled worker launcher is not executable: $RESOURCES_DIR/workers/benchforge-worker" >&2
  exit 1
fi

BUNDLE_EXECUTABLE="$(plutil -extract CFBundleExecutable raw -o - "$INFO_PLIST")"
EXECUTABLE_PATH="$APP_PATH/Contents/MacOS/$BUNDLE_EXECUTABLE"
require_path "$EXECUTABLE_PATH" "app executable"
if [[ ! -x "$EXECUTABLE_PATH" ]]; then
  echo "App executable is not executable: $EXECUTABLE_PATH" >&2
  exit 1
fi

(
  cd "$INSTALL_DIR"
  HOME="$HOME_DIR" "$EXECUTABLE_PATH" --benchforge-path-smoke
) >"$PATH_LOG_PATH" 2>&1 || {
  status=$?
  echo "Install path smoke failed with status $status. Log: $PATH_LOG_PATH" >&2
  tail -80 "$PATH_LOG_PATH" >&2
  exit "$status"
}

python3 - "$PATH_LOG_PATH" "$RESOURCES_DIR" "$HOME_DIR" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
expected_resource_dir = Path(sys.argv[2]).resolve()
expected_data_dir = Path(sys.argv[3]) / "Library" / "Application Support" / "BenchForge"

try:
    payload = json.loads(log_path.read_text(encoding="utf-8"))
except json.JSONDecodeError as exc:
    raise SystemExit(f"install path smoke did not emit JSON: {exc}; log: {log_path}") from exc

if payload.get("status") != "ok":
    raise SystemExit(f"install path smoke returned status {payload.get('status')!r}; log: {log_path}")

actual_resource_dir = Path(payload.get("resourceDir", "")).resolve()
if actual_resource_dir != expected_resource_dir:
    raise SystemExit(
        "install path smoke used the wrong resource directory: "
        f"{actual_resource_dir} != {expected_resource_dir}; log: {log_path}"
    )

actual_worker_launcher = Path(payload.get("workerLauncher", "")).resolve()
expected_worker_launcher = expected_resource_dir / "workers" / "benchforge-worker"
if actual_worker_launcher != expected_worker_launcher.resolve():
    raise SystemExit(
        "install path smoke reported the wrong bundled worker launcher: "
        f"{actual_worker_launcher} != {expected_worker_launcher.resolve()}; log: {log_path}"
    )

actual_worker_package = Path(payload.get("workerPackageDir", "")).resolve()
expected_worker_package = expected_resource_dir / "workers" / "benchforge_worker"
if actual_worker_package != expected_worker_package.resolve():
    raise SystemExit(
        "install path smoke reported the wrong bundled worker package: "
        f"{actual_worker_package} != {expected_worker_package.resolve()}; log: {log_path}"
    )

actual_data_dir = Path(payload.get("dataDir", "")).resolve()
if actual_data_dir != expected_data_dir.resolve():
    raise SystemExit(
        "install path smoke used the wrong default data directory: "
        f"{actual_data_dir} != {expected_data_dir.resolve()}; log: {log_path}"
    )

if payload.get("dataDirOverride") is not None:
    raise SystemExit(f"install path smoke unexpectedly used BENCHFORGE_DATA_DIR; log: {log_path}")

print(f"ok   installed app default data dir: {actual_data_dir}")
print(f"ok   installed app path smoke log: {log_path}")
PY

(
  cd "$INSTALL_DIR"
  HOME="$HOME_DIR" BENCHFORGE_DATA_DIR="$DATA_DIR" "$EXECUTABLE_PATH" --benchforge-first-run-smoke
) >"$LOG_PATH" 2>&1 || {
  status=$?
  echo "Install smoke failed with status $status. Log: $LOG_PATH" >&2
  tail -120 "$LOG_PATH" >&2
  exit "$status"
}

python3 - "$LOG_PATH" "$RESOURCES_DIR" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
expected_resource_dir = Path(sys.argv[2]).resolve()

try:
    payload = json.loads(log_path.read_text(encoding="utf-8"))
except json.JSONDecodeError as exc:
    raise SystemExit(f"install smoke did not emit JSON: {exc}; log: {log_path}") from exc

status = payload.get("status")
if status != "ok":
    raise SystemExit(f"install smoke returned status {status!r}; log: {log_path}")

resource_dir = payload.get("resourceDir")
if not resource_dir:
    raise SystemExit(f"install smoke did not report resourceDir; log: {log_path}")

actual_resource_dir = Path(resource_dir).resolve()
if actual_resource_dir != expected_resource_dir:
    raise SystemExit(
        "install smoke used the wrong resource directory: "
        f"{actual_resource_dir} != {expected_resource_dir}; log: {log_path}"
    )

result_count = int(payload.get("resultCount") or 0)
if result_count < 1:
    raise SystemExit(f"install smoke did not create benchmark results; log: {log_path}")

print(f"ok   installed app first-run smoke completed with {result_count} result(s)")
print(f"ok   bundled resources used: {actual_resource_dir}")
print(f"ok   log: {log_path}")
PY

(
  cd "$INSTALL_DIR"
  HOME="$HOME_DIR" BENCHFORGE_DATA_DIR="$DATA_DIR" "$EXECUTABLE_PATH" --benchforge-worker-mock
) >"$WORKER_LOG_PATH" 2>&1 || {
  status=$?
  echo "Install worker smoke failed with status $status. Log: $WORKER_LOG_PATH" >&2
  tail -120 "$WORKER_LOG_PATH" >&2
  exit "$status"
}

python3 - "$WORKER_LOG_PATH" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
try:
    payload = json.loads(log_path.read_text(encoding="utf-8"))
except json.JSONDecodeError as exc:
    raise SystemExit(f"install worker smoke did not emit JSON: {exc}; log: {log_path}") from exc

if payload.get("status") != "passed":
    raise SystemExit(f"install worker smoke returned status {payload.get('status')!r}; log: {log_path}")
if payload.get("targetId") != "benchforge-worker":
    raise SystemExit(f"install worker smoke used unexpected target {payload.get('targetId')!r}; log: {log_path}")

print(f"ok   bundled worker executed through installed app: {log_path}")
PY

(
  cd "$INSTALL_DIR"
  HOME="$HOME_DIR" BENCHFORGE_DATA_DIR="$DATA_DIR" "$EXECUTABLE_PATH" --benchforge-security-smoke
) >"$SECURITY_LOG_PATH" 2>&1 || {
  status=$?
  echo "Install security smoke failed with status $status. Log: $SECURITY_LOG_PATH" >&2
  tail -160 "$SECURITY_LOG_PATH" >&2
  exit "$status"
}

python3 - "$SECURITY_LOG_PATH" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
try:
    payload = json.loads(log_path.read_text(encoding="utf-8"))
except json.JSONDecodeError as exc:
    raise SystemExit(f"install security smoke did not emit JSON: {exc}; log: {log_path}") from exc

if not isinstance(payload, list):
    raise SystemExit(f"install security smoke did not emit a result list; log: {log_path}")
if len(payload) != 4:
    raise SystemExit(f"install security smoke expected 4 results, got {len(payload)}; log: {log_path}")

bad = [
    item for item in payload
    if item.get("benchmarkPackId") != "security-defensive"
    or item.get("targetId") != "benchforge-worker"
    or item.get("status") != "passed"
]
if bad:
    raise SystemExit(f"install security smoke had unexpected results: {bad!r}; log: {log_path}")

print(f"ok   bundled worker ran security-defensive pack through installed app: {log_path}")
PY
