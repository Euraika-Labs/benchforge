#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DMG_PATH="${1:-}"
EXPECTED_NAME="${BENCHFORGE_EXPECTED_PRODUCT_NAME:-BenchForge}"
EXPECTED_IDENTIFIER="${BENCHFORGE_EXPECTED_BUNDLE_ID:-com.benchforge.desktop}"
EXPECTED_VERSION="${BENCHFORGE_EXPECTED_VERSION:-}"
REQUIRE_CODESIGN="${BENCHFORGE_REQUIRE_CODESIGN:-0}"
REQUIRE_DEVELOPER_ID="${BENCHFORGE_REQUIRE_DEVELOPER_ID:-0}"
REQUIRE_NOTARIZATION="${BENCHFORGE_REQUIRE_NOTARIZATION:-0}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "DMG verification requires macOS." >&2
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

if [[ -z "$EXPECTED_VERSION" ]]; then
  EXPECTED_VERSION="$(
    python3 - "$ROOT/app-scaffold/src-tauri/tauri.conf.json" <<'PY'
import json
import sys
from pathlib import Path

print(json.loads(Path(sys.argv[1]).read_text())["version"])
PY
  )"
fi

echo "Verifying DMG checksum: $DMG_PATH"
hdiutil verify "$DMG_PATH" >/dev/null

MOUNT_POINT="$(mktemp -d "${TMPDIR:-/tmp}/benchforge-dmg.XXXXXX")"
ATTACHED=0
cleanup() {
  if [[ "$ATTACHED" == "1" ]]; then
    hdiutil detach "$MOUNT_POINT" >/dev/null 2>&1 || true
  fi
  rmdir "$MOUNT_POINT" >/dev/null 2>&1 || true
}
trap cleanup EXIT

hdiutil attach -quiet -readonly -nobrowse -noautoopen -mountpoint "$MOUNT_POINT" "$DMG_PATH"
ATTACHED=1

APP_PATH="$MOUNT_POINT/$EXPECTED_NAME.app"
INFO_PLIST="$APP_PATH/Contents/Info.plist"

require_path() {
  local path="$1"
  local description="$2"
  if [[ ! -e "$path" ]]; then
    echo "Missing $description: $path" >&2
    exit 1
  fi
}

plist_value() {
  local key="$1"
  plutil -extract "$key" raw -o - "$INFO_PLIST"
}

require_path "$APP_PATH" "app bundle"
require_path "$INFO_PLIST" "Info.plist"
require_path "$MOUNT_POINT/Applications" "Applications shortcut"

BUNDLE_NAME="$(plist_value CFBundleName)"
DISPLAY_NAME="$(plist_value CFBundleDisplayName)"
BUNDLE_IDENTIFIER="$(plist_value CFBundleIdentifier)"
BUNDLE_VERSION="$(plist_value CFBundleShortVersionString)"
BUNDLE_EXECUTABLE="$(plist_value CFBundleExecutable)"
MIN_SYSTEM="$(plist_value LSMinimumSystemVersion)"

if [[ "$BUNDLE_NAME" != "$EXPECTED_NAME" || "$DISPLAY_NAME" != "$EXPECTED_NAME" ]]; then
  echo "Unexpected bundle name/display name: $BUNDLE_NAME / $DISPLAY_NAME" >&2
  exit 1
fi
if [[ "$BUNDLE_IDENTIFIER" != "$EXPECTED_IDENTIFIER" ]]; then
  echo "Unexpected bundle identifier: $BUNDLE_IDENTIFIER" >&2
  exit 1
fi
if [[ "$BUNDLE_VERSION" != "$EXPECTED_VERSION" ]]; then
  echo "Unexpected bundle version: $BUNDLE_VERSION, expected $EXPECTED_VERSION" >&2
  exit 1
fi
if [[ "$MIN_SYSTEM" != "13.0" ]]; then
  echo "Unexpected minimum macOS version: $MIN_SYSTEM" >&2
  exit 1
fi

EXECUTABLE_PATH="$APP_PATH/Contents/MacOS/$BUNDLE_EXECUTABLE"
require_path "$EXECUTABLE_PATH" "app executable"
if [[ ! -x "$EXECUTABLE_PATH" ]]; then
  echo "App executable is not executable: $EXECUTABLE_PATH" >&2
  exit 1
fi

if [[ ! -d "$APP_PATH/Contents/Resources" ]]; then
  echo "Missing app resources directory: $APP_PATH/Contents/Resources" >&2
  exit 1
fi
for resource in adapters benchmark-packs docker fixtures workers/benchforge_worker; do
  if [[ ! -d "$APP_PATH/Contents/Resources/$resource" ]]; then
    echo "Missing bundled $resource resource: $APP_PATH/Contents/Resources/$resource" >&2
    exit 1
  fi
done
if [[ ! -x "$APP_PATH/Contents/Resources/workers/benchforge-worker" ]]; then
  echo "Missing executable bundled worker launcher: $APP_PATH/Contents/Resources/workers/benchforge-worker" >&2
  exit 1
fi

CODESIGN_VERIFY_LOG="$(mktemp "${TMPDIR:-/tmp}/benchforge-codesign.XXXXXX")"
CODESIGN_DETAIL_LOG="$(mktemp "${TMPDIR:-/tmp}/benchforge-codesign-detail.XXXXXX")"
SPCTL_LOG="$(mktemp "${TMPDIR:-/tmp}/benchforge-spctl.XXXXXX")"
STAPLER_LOG="$(mktemp "${TMPDIR:-/tmp}/benchforge-stapler.XXXXXX")"

if codesign --verify --deep --strict "$APP_PATH" >"$CODESIGN_VERIFY_LOG" 2>&1; then
  echo "ok   codesign verification passed"
else
  if [[ "$REQUIRE_CODESIGN" == "1" ]]; then
    cat "$CODESIGN_VERIFY_LOG" >&2
    exit 1
  fi
  echo "warn codesign verification did not pass for unsigned/dev bundle:"
  sed 's/^/     /' "$CODESIGN_VERIFY_LOG"
fi

if codesign -dv --verbose=4 "$APP_PATH" >"$CODESIGN_DETAIL_LOG" 2>&1; then
  if [[ "$REQUIRE_DEVELOPER_ID" == "1" ]] && ! grep -q "Authority=Developer ID Application:" "$CODESIGN_DETAIL_LOG"; then
    echo "App is signed, but not with a Developer ID Application certificate:" >&2
    sed 's/^/     /' "$CODESIGN_DETAIL_LOG" >&2
    exit 1
  fi
elif [[ "$REQUIRE_DEVELOPER_ID" == "1" ]]; then
  cat "$CODESIGN_DETAIL_LOG" >&2
  exit 1
fi

if [[ "$REQUIRE_NOTARIZATION" == "1" ]]; then
  if ! command -v spctl >/dev/null 2>&1; then
    echo "spctl is required for notarization verification." >&2
    exit 1
  fi
  if ! xcrun -f stapler >/dev/null 2>&1; then
    echo "xcrun stapler is required for notarization verification." >&2
    exit 1
  fi
  if spctl --assess --type execute --verbose=4 "$APP_PATH" >"$SPCTL_LOG" 2>&1; then
    echo "ok   Gatekeeper assessment passed"
  else
    cat "$SPCTL_LOG" >&2
    exit 1
  fi
  if xcrun stapler validate "$APP_PATH" >"$STAPLER_LOG" 2>&1; then
    echo "ok   notarization ticket validation passed"
  else
    cat "$STAPLER_LOG" >&2
    exit 1
  fi
fi

echo "ok   DMG contains $EXPECTED_NAME.app $BUNDLE_VERSION ($BUNDLE_IDENTIFIER)"
echo "ok   app executable: Contents/MacOS/$BUNDLE_EXECUTABLE"
echo "ok   bundled resources: adapters, benchmark-packs, docker, fixtures, workers"
echo "ok   $(shasum -a 256 "$DMG_PATH")"
