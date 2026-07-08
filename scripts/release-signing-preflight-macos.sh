#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_CONF="$ROOT/app-scaffold/src-tauri/tauri.conf.json"
RELEASE_DISTRIBUTION="${BENCHFORGE_RELEASE_DISTRIBUTION:-${BENCHFORGE_REQUIRE_CODESIGN:-0}}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS signing preflight requires macOS." >&2
  exit 1
fi

if [[ "$RELEASE_DISTRIBUTION" != "1" ]]; then
  echo "ok   signing preflight skipped for unsigned local build"
  echo "     Set BENCHFORGE_RELEASE_DISTRIBUTION=1 to require Developer ID signing and notarization inputs."
  exit 0
fi

errors=()

present() {
  local name="$1"
  [[ -n "${!name:-}" ]]
}

safe_state() {
  local name="$1"
  if present "$name"; then
    echo "<set>"
  else
    echo "<unset>"
  fi
}

require_command() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    errors+=("missing required command: $name")
  fi
}

require_xcrun_tool() {
  local name="$1"
  if ! xcrun -f "$name" >/dev/null 2>&1; then
    errors+=("missing Xcode command line tool: $name")
  fi
}

require_command python3
require_command codesign
require_command security
require_command spctl
require_command xcrun

require_xcrun_tool notarytool
require_xcrun_tool stapler

CONFIG_SIGNING_IDENTITY="$(
  python3 - "$TAURI_CONF" <<'PY'
import json
import sys
from pathlib import Path

conf = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
macos = conf.get("bundle", {}).get("macOS", {})
print(macos.get("signingIdentity") or "")
PY
)"

while IFS= read -r config_error; do
  [[ -n "$config_error" ]] && errors+=("$config_error")
done < <(
  python3 - "$TAURI_CONF" <<'PY'
import json
import sys
from pathlib import Path

conf = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
macos = conf.get("bundle", {}).get("macOS", {})
identity = macos.get("signingIdentity")
if macos.get("hardenedRuntime", True) is not True:
    print("Tauri bundle.macOS.hardenedRuntime must be true for public distribution")
if identity == "-":
    print("Tauri bundle.macOS.signingIdentity is ad-hoc ('-'), which is not valid for public distribution")
PY
)

SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-$CONFIG_SIGNING_IDENTITY}"
if present APPLE_CERTIFICATE; then
  if ! present APPLE_CERTIFICATE_PASSWORD; then
    errors+=("APPLE_CERTIFICATE_PASSWORD is required when APPLE_CERTIFICATE is set")
  fi
elif [[ -n "$SIGNING_IDENTITY" ]]; then
  if ! security find-identity -v -p codesigning | grep -F "\"$SIGNING_IDENTITY\"" >/dev/null; then
    errors+=("code signing identity not found in keychain: $SIGNING_IDENTITY")
  fi
else
  errors+=("set APPLE_SIGNING_IDENTITY or APPLE_CERTIFICATE for Developer ID code signing")
fi

APPLE_ID_AUTH=0
API_KEY_AUTH=0
if present APPLE_ID || present APPLE_PASSWORD || present APPLE_TEAM_ID; then
  APPLE_ID_AUTH=1
  present APPLE_ID || errors+=("APPLE_ID is required for Apple ID notarization")
  present APPLE_PASSWORD || errors+=("APPLE_PASSWORD is required for Apple ID notarization")
  present APPLE_TEAM_ID || errors+=("APPLE_TEAM_ID is required for Apple ID notarization")
fi

if present APPLE_API_KEY || present APPLE_API_ISSUER || present APPLE_API_KEY_PATH || present API_PRIVATE_KEYS_DIR; then
  API_KEY_AUTH=1
  present APPLE_API_KEY || errors+=("APPLE_API_KEY is required for App Store Connect API notarization")
  present APPLE_API_ISSUER || errors+=("APPLE_API_ISSUER is required for App Store Connect API notarization")
  if present APPLE_API_KEY_PATH; then
    [[ -f "$APPLE_API_KEY_PATH" ]] || errors+=("APPLE_API_KEY_PATH does not point to a file: $APPLE_API_KEY_PATH")
  elif present APPLE_API_KEY; then
    key_file="AuthKey_${APPLE_API_KEY}.p8"
    key_found=0
    key_dirs=()
    if present API_PRIVATE_KEYS_DIR; then
      key_dirs+=("$API_PRIVATE_KEYS_DIR")
    fi
    key_dirs+=("$ROOT/app-scaffold/private_keys" "$HOME/private_keys" "$HOME/.private_keys" "$HOME/.appstoreconnect/private_keys")
    for dir in "${key_dirs[@]}"; do
      if [[ -f "$dir/$key_file" ]]; then
        key_found=1
        break
      fi
    done
    if [[ "$key_found" != "1" ]]; then
      errors+=("App Store Connect API key file not found; set APPLE_API_KEY_PATH or place $key_file in a supported private_keys directory")
    fi
  fi
fi

if [[ "$APPLE_ID_AUTH" == "0" && "$API_KEY_AUTH" == "0" ]]; then
  errors+=("configure notarization with APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID or APPLE_API_KEY/APPLE_API_ISSUER")
fi

echo "Release signing/notarization preflight:"
echo "  APPLE_SIGNING_IDENTITY=${APPLE_SIGNING_IDENTITY:-${CONFIG_SIGNING_IDENTITY:-<unset>}}"
echo "  APPLE_CERTIFICATE=$(safe_state APPLE_CERTIFICATE)"
echo "  APPLE_ID=$(safe_state APPLE_ID)"
echo "  APPLE_TEAM_ID=$(safe_state APPLE_TEAM_ID)"
echo "  APPLE_PASSWORD=$(safe_state APPLE_PASSWORD)"
echo "  APPLE_API_KEY=$(safe_state APPLE_API_KEY)"
echo "  APPLE_API_ISSUER=$(safe_state APPLE_API_ISSUER)"
echo "  APPLE_API_KEY_PATH=${APPLE_API_KEY_PATH:-<unset>}"
echo "  APPLE_PROVIDER_SHORT_NAME=${APPLE_PROVIDER_SHORT_NAME:-<unset>}"

if (( ${#errors[@]} > 0 )); then
  for error in "${errors[@]}"; do
    echo "fail $error" >&2
  done
  exit 1
fi

echo "ok   release signing preflight passed"
