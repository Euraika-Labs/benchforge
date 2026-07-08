#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DMG_PATH="${1:-}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS distribution verification requires macOS." >&2
  exit 1
fi

if [[ -n "$DMG_PATH" ]]; then
  BENCHFORGE_REQUIRE_CODESIGN=1 \
  BENCHFORGE_REQUIRE_DEVELOPER_ID=1 \
  BENCHFORGE_REQUIRE_NOTARIZATION=1 \
    "$ROOT/scripts/verify-dmg-macos.sh" "$DMG_PATH"
else
  BENCHFORGE_REQUIRE_CODESIGN=1 \
  BENCHFORGE_REQUIRE_DEVELOPER_ID=1 \
  BENCHFORGE_REQUIRE_NOTARIZATION=1 \
    "$ROOT/scripts/verify-dmg-macos.sh"
fi
