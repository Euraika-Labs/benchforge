#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$ROOT/.." && pwd)"
PYTHON_BIN="${BENCHFORGE_PYTHON:-${PYTHON:-python3}}"

if [[ ! -f "$REPO_ROOT/LICENSE" && -f "$ROOT/LICENSE" ]]; then
  REPO_ROOT="$ROOT"
fi

cd "$ROOT"
"$ROOT/scripts/generate-placeholder-icon.py" >/dev/null

"$PYTHON_BIN" - "$ROOT" "$REPO_ROOT" <<'PY'
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

try:
    import yaml
except ImportError as exc:
    raise SystemExit("PyYAML is required for release preflight; run ./scripts/bootstrap.sh first") from exc

root = Path(sys.argv[1])
repo_root = Path(sys.argv[2])
errors: list[str] = []
warnings: list[str] = []


def require_file(path: Path, description: str) -> str:
    if not path.is_file():
        errors.append(f"missing {description}: {path}")
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


def require_contains(path: Path, needle: str, description: str) -> None:
    text = require_file(path, description)
    if text and needle not in text:
        errors.append(f"{description} does not mention {needle!r}: {path}")


license_text = require_file(repo_root / "LICENSE", "Apache-2.0 license")
if license_text:
    if "Apache License" not in license_text or "Version 2.0" not in license_text:
        errors.append("LICENSE must be Apache License 2.0")
    if "Copyright 2026" not in license_text:
        warnings.append("LICENSE does not contain a 2026 copyright notice")

for rel, description in [
    ("README.md", "root README"),
    ("CONTRIBUTING.md", "contributing guide"),
    ("SECURITY.md", "security policy"),
    ("CODE_OF_CONDUCT.md", "code of conduct"),
    (".github/PULL_REQUEST_TEMPLATE.md", "pull request template"),
    (".github/ISSUE_TEMPLATE/bug_report.yml", "bug issue template"),
    (".github/ISSUE_TEMPLATE/feature_request.yml", "feature issue template"),
    (".github/ISSUE_TEMPLATE/config.yml", "issue template config"),
]:
    require_file(repo_root / rel, description)
for rel, description in [
    ("scripts/package-dmg-macos.sh", "DMG packaging script"),
    ("scripts/verify-dmg-macos.sh", "DMG verification script"),
    ("scripts/verify-dmg-install-smoke-macos.sh", "installed DMG smoke script"),
    ("scripts/product-readiness.sh", "product readiness script"),
    ("scripts/release-signing-preflight-macos.sh", "macOS signing preflight script"),
    ("scripts/verify-macos-distribution.sh", "macOS distribution verification script"),
]:
    require_file(root / rel, description)

require_contains(repo_root / "README.md", "Apache License 2.0", "root README")
require_contains(repo_root / "README.md", "CONTRIBUTING.md", "root README")
require_contains(repo_root / "README.md", "SECURITY.md", "root README")
require_contains(repo_root / "README.md", "make verify-distribution-dmg", "root README")
require_contains(root / "README.md", "Apache License 2.0", "app README")
app_contributing_ref = "../CONTRIBUTING.md" if root != repo_root else "CONTRIBUTING.md"
require_contains(root / "README.md", app_contributing_ref, "app README")
require_contains(root / "README.md", "make verify-distribution-dmg", "app README")

for template in sorted((repo_root / ".github" / "ISSUE_TEMPLATE").glob("*.yml")):
    try:
        parsed = yaml.safe_load(template.read_text(encoding="utf-8"))
    except Exception as exc:  # noqa: BLE001 - report exact template path.
        errors.append(f"invalid GitHub issue template YAML {template}: {exc}")
        continue
    if not isinstance(parsed, dict):
        errors.append(f"GitHub issue template must be a mapping: {template}")
        continue
    if template.name != "config.yml":
        for key in ("name", "description", "body"):
            if key not in parsed:
                errors.append(f"GitHub issue template missing {key}: {template}")

ci_workflow = repo_root / ".github" / "workflows" / "ci.yml"
ci_text = require_file(ci_workflow, "GitHub Actions CI workflow")
if ci_text:
    try:
        ci_parsed = yaml.safe_load(ci_text)
    except Exception as exc:  # noqa: BLE001 - report exact workflow path.
        errors.append(f"invalid GitHub Actions workflow YAML {ci_workflow}: {exc}")
        ci_parsed = {}
    if not isinstance(ci_parsed, dict):
        errors.append(f"GitHub Actions workflow must be a mapping: {ci_workflow}")
        ci_parsed = {}
    jobs = ci_parsed.get("jobs") if isinstance(ci_parsed.get("jobs"), dict) else {}
    if not jobs:
        errors.append("GitHub Actions CI workflow must define at least one job")
    runs_on_values = [
        str(job.get("runs-on", ""))
        for job in jobs.values()
        if isinstance(job, dict)
    ]
    if not any("macos" in value.lower() for value in runs_on_values):
        errors.append("GitHub Actions CI workflow must include a macOS job for the Tauri app")
    run_commands: list[str] = []
    for job in jobs.values():
        if not isinstance(job, dict):
            continue
        for step in job.get("steps", []):
            if isinstance(step, dict) and isinstance(step.get("run"), str):
                run_commands.append(step["run"])
    joined_runs = "\n".join(run_commands)
    for command in ("./scripts/bootstrap.sh", "make doctor", "make dependency-audit", "make test", "make benchmark-readiness"):
        if command not in joined_runs:
            errors.append(f"GitHub Actions CI workflow must run {command!r}")
    if "brew install llama.cpp" not in joined_runs:
        errors.append("GitHub Actions CI workflow must install llama.cpp for the Hugging Face local readiness gate")

package_json = json.loads(require_file(root / "app-scaffold" / "package.json", "web package manifest") or "{}")
if package_json.get("private") is not True:
    warnings.append("app-scaffold/package.json should remain private unless publishing the npm package intentionally")
if "tauri:build:dmg" not in package_json.get("scripts", {}):
    errors.append("app-scaffold/package.json must define scripts.tauri:build:dmg")
if not (root / "app-scaffold" / "package-lock.json").is_file():
    errors.append("app-scaffold/package-lock.json is required for reproducible release installs")

tauri_conf = json.loads(require_file(root / "app-scaffold" / "src-tauri" / "tauri.conf.json", "Tauri config") or "{}")
if tauri_conf.get("productName") != "BenchForge":
    errors.append("Tauri productName must be BenchForge")
if not re.fullmatch(r"[a-z][a-z0-9]*(\.[a-z][a-z0-9-]*){2,}", str(tauri_conf.get("identifier", ""))):
    errors.append("Tauri identifier must be a stable reverse-DNS identifier")
if not re.fullmatch(r"\d+\.\d+\.\d+([-.][A-Za-z0-9.]+)?", str(tauri_conf.get("version", ""))):
    errors.append("Tauri version must look like semantic versioning")
bundle = tauri_conf.get("bundle") if isinstance(tauri_conf.get("bundle"), dict) else {}
targets = bundle.get("targets") if isinstance(bundle.get("targets"), list) else []
if "dmg" not in targets:
    errors.append("Tauri bundle.targets must include dmg")
macos = bundle.get("macOS") if isinstance(bundle.get("macOS"), dict) else {}
if macos.get("hardenedRuntime") is not True:
    errors.append("Tauri bundle.macOS.hardenedRuntime must be true for distributable macOS builds")
if macos.get("signingIdentity") == "-":
    warnings.append("Tauri bundle.macOS.signingIdentity is ad-hoc; public distribution must use Developer ID signing")
resources = bundle.get("resources") if isinstance(bundle.get("resources"), dict) else {}
for source, destination in {
    "../../adapters": "adapters",
    "../../benchmark-packs": "benchmark-packs",
    "../../docker": "docker",
    "../../fixtures": "fixtures",
    "../../workers/benchforge-worker": "workers/benchforge-worker",
    "../../workers/benchforge_worker/__init__.py": "workers/benchforge_worker/__init__.py",
    "../../workers/benchforge_worker/aider_runner.py": "workers/benchforge_worker/aider_runner.py",
    "../../workers/benchforge_worker/cli.py": "workers/benchforge_worker/cli.py",
    "../../workers/benchforge_worker/evalplus_runner.py": "workers/benchforge_worker/evalplus_runner.py",
    "../../workers/benchforge_worker/harness_runner.py": "workers/benchforge_worker/harness_runner.py",
    "../../workers/benchforge_worker/schemas.py": "workers/benchforge_worker/schemas.py",
    "../../workers/benchforge_worker/scoring.py": "workers/benchforge_worker/scoring.py",
    "../../workers/benchforge_worker/security_runner.py": "workers/benchforge_worker/security_runner.py",
    "../../workers/benchforge_worker/swebench_runner.py": "workers/benchforge_worker/swebench_runner.py",
    "../../workers/benchforge_worker/terminal_bench_runner.py": "workers/benchforge_worker/terminal_bench_runner.py",
}.items():
    if resources.get(source) != destination:
        errors.append(f"Tauri bundle.resources must map {source!r} to {destination!r}")
for icon in bundle.get("icon", []):
    icon_path = root / "app-scaffold" / "src-tauri" / icon
    if not icon_path.is_file():
        errors.append(f"Tauri bundle icon is missing: {icon_path}")

stale_phrases = [
    "LICENSE-PLACEHOLDER",
    "choose and commit a real license",
    "Do not ship this placeholder",
]
scan_roots = [
    repo_root / "README.md",
    repo_root / "CONTRIBUTING.md",
    repo_root / "SECURITY.md",
    repo_root / "CODE_OF_CONDUCT.md",
    repo_root / ".github",
    root / "README.md",
    root / "docs",
    root / "scripts",
    root / "app-scaffold" / "package.json",
    root / "app-scaffold" / "src-tauri" / "tauri.conf.json",
]
for scan_root in scan_roots:
    if scan_root.is_file():
        candidates = [scan_root]
    elif scan_root.is_dir():
        candidates = [path for path in scan_root.rglob("*") if path.is_file()]
    else:
        continue
    for path in candidates:
        if path == root / "scripts" / "release-preflight.sh":
            continue
        if any(part in {"node_modules", "target", ".benchforge", "dist"} for part in path.parts):
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for phrase in stale_phrases:
            if phrase in text:
                errors.append(f"stale release placeholder phrase {phrase!r} in {path}")

for warning in warnings:
    print(f"warn {warning}")

if errors:
    for error in errors:
        print(f"fail {error}", file=sys.stderr)
    raise SystemExit(1)

print("ok   release preflight passed")
PY
