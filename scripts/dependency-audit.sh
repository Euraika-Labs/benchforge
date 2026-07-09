#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/app-scaffold/src-tauri/Cargo.toml"
ADVISORY="GHSA-wrw7-89jp-8q8g"

echo "BenchForge dependency audit"
echo "Supported desktop release target: macOS"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  echo "Checking $target for glib advisory $ADVISORY..."
  output="$(cargo tree --manifest-path "$MANIFEST" --target "$target" -i glib 2>&1 || true)"
  if printf '%s\n' "$output" | grep -Eq '^glib v'; then
    printf '%s\n' "$output"
    echo "error: glib is present in the supported macOS dependency graph" >&2
    exit 1
  fi
  echo "ok   $target: glib is not in the supported macOS dependency graph"
done

all_target_output="$(cargo tree --manifest-path "$MANIFEST" --target all -i glib 2>&1 || true)"
if printf '%s\n' "$all_target_output" | grep -Eq '^glib v0\.18\.'; then
  echo "note glib 0.18 remains in Cargo.lock through Tauri's GTK/Linux dependency graph."
  echo "note BenchForge CI, Keychain integration, and DMG release targets are macOS-only."
fi
