#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "DMG packaging requires macOS." >&2
  exit 1
fi
cd "$ROOT/app-scaffold"
"$ROOT/scripts/generate-placeholder-icon.py" >/dev/null
"$ROOT/scripts/release-signing-preflight-macos.sh"
npm ci
echo "Signing/notarization environment:"
echo "  BENCHFORGE_RELEASE_DISTRIBUTION=${BENCHFORGE_RELEASE_DISTRIBUTION:-0}"
echo "  APPLE_SIGNING_IDENTITY=${APPLE_SIGNING_IDENTITY:-<unset>}"
if [[ -n "${APPLE_CERTIFICATE:-}" ]]; then
  echo "  APPLE_CERTIFICATE=<set>"
else
  echo "  APPLE_CERTIFICATE=<unset>"
fi
echo "  APPLE_ID=${APPLE_ID:-<unset>}"
echo "  APPLE_TEAM_ID=${APPLE_TEAM_ID:-<unset>}"
if [[ -n "${APPLE_PASSWORD:-}" ]]; then
  echo "  APPLE_PASSWORD=<set>"
else
  echo "  APPLE_PASSWORD=<unset>"
fi
if [[ -n "${APPLE_API_KEY:-}" ]]; then
  echo "  APPLE_API_KEY=<set>"
else
  echo "  APPLE_API_KEY=<unset>"
fi
if [[ -n "${APPLE_API_ISSUER:-}" ]]; then
  echo "  APPLE_API_ISSUER=<set>"
else
  echo "  APPLE_API_ISSUER=<unset>"
fi
npm run tauri:build:dmg
DMG_DIR="$ROOT/app-scaffold/src-tauri/target/release/bundle/dmg"
if compgen -G "$DMG_DIR/*.dmg" >/dev/null; then
  echo "Built DMG:"
  for dmg in "$DMG_DIR"/*.dmg; do
    echo "$dmg"
    if [[ "${BENCHFORGE_RELEASE_DISTRIBUTION:-0}" == "1" ]]; then
      "$ROOT/scripts/verify-macos-distribution.sh" "$dmg"
    else
      "$ROOT/scripts/verify-dmg-macos.sh" "$dmg"
    fi
  done
fi
