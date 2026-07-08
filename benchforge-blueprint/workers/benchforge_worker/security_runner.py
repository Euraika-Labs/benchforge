from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import tempfile
import time
import uuid
from pathlib import Path


IGNORED_DIRS = {".git", ".benchforge-venv", ".venv", "node_modules", "__pycache__", "target"}
SCANNED_SUFFIXES = {".py", ".js", ".jsx", ".ts", ".tsx", ".go", ".rs", ".java", ".rb", ".php", ".sh", ".yaml", ".yml", ".json", ".env", ".toml", ".ini", ".cfg"}
SCANNED_FILE_NAMES = {".env", ".env.local", ".npmrc", ".pypirc", "credentials"}
DEPENDENCY_MANIFEST_NAMES = {"requirements.txt", "pyproject.toml", "package.json", "package-lock.json", "npm-shrinkwrap.json"}
PYTHON_DEPENDENCY_RULES = {
    "django": ("3.2.25", "Django dependency is below the current supported security patch floor"),
    "flask": ("2.2.5", "Flask dependency is below the security patch floor"),
    "jinja2": ("3.1.4", "Jinja2 dependency is below the security patch floor"),
    "pyyaml": ("5.4.1", "PyYAML dependency is below the safe loader security patch floor"),
    "requests": ("2.32.0", "Requests dependency is below the recent security patch floor"),
}
JS_DEPENDENCY_RULES = {
    "lodash": ("4.17.21", "lodash dependency is below the prototype-pollution patch floor"),
    "minimist": ("1.2.8", "minimist dependency is below the prototype-pollution patch floor"),
    "serialize-javascript": ("6.0.2", "serialize-javascript dependency is below the XSS patch floor"),
}
SECRET_PATTERNS = [
    ("secret-openai-key", "Possible OpenAI API key", "critical", re.compile(r"\bsk-[A-Za-z0-9][A-Za-z0-9_-]{20,}\b")),
    ("secret-huggingface-token", "Possible Hugging Face token", "critical", re.compile(r"\bhf_[A-Za-z0-9]{20,}\b")),
    ("secret-github-token", "Possible GitHub token", "critical", re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{30,}\b")),
    ("secret-aws-access-key", "Possible AWS access key", "critical", re.compile(r"\bA[KS]IA[0-9A-Z]{16}\b")),
    ("secret-private-key", "Private key material marker", "critical", re.compile(r"BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY")),
    (
        "secret-generic-assignment",
        "Possible hard-coded secret assignment",
        "high",
        re.compile(r"(?i)\b(?:api[_-]?key|secret|token|password)\b\s*[:=]\s*['\"][^'\"\s]{12,}['\"]"),
    ),
]
FALLBACK_RULES = [
    ("python-eval", "Use of eval()", "high", "eval("),
    ("python-exec", "Use of exec()", "high", "exec("),
    ("python-os-system", "Shell execution through os.system", "high", "os.system("),
    ("python-shell-true", "Subprocess shell=True", "high", "shell=True"),
    ("python-pickle-loads", "Unsafe pickle deserialization", "high", "pickle.loads("),
    ("python-yaml-load", "Potential unsafe yaml.load", "medium", "yaml.load("),
    ("js-child-process-exec", "Shell execution through child_process.exec", "high", "child_process.exec("),
    ("js-inner-html", "Direct innerHTML assignment", "medium", ".innerHTML"),
    ("generic-private-key", "Embedded private key marker", "critical", "BEGIN PRIVATE KEY"),
]


def emit(event: dict) -> None:
    print(json.dumps(event, ensure_ascii=False), flush=True)


def run(args: argparse.Namespace) -> int:
    run_id = args.run_id or str(uuid.uuid4())
    workspace = Path(args.workspace or ".").resolve()
    output = Path(args.output).resolve() if args.output else None
    events: list[dict] = []
    started = time.monotonic()

    def record(event: dict) -> None:
        events.append(event)
        emit(event)

    record({"type": "run_started", "run_id": run_id, "kind": "security", "tool": args.tool or "fallback", "workspace": str(workspace), "timestamp": time.time()})
    if not workspace.exists() or not workspace.is_dir():
        result = finish_event(run_id, "error", None, started, [], 0, [{"level": "error", "message": f"workspace does not exist: {workspace}"}])
        record(result)
        write_output(output, events)
        return 2

    diagnostics: list[dict] = []
    findings: list[dict]
    files_scanned: int
    if args.tool == "semgrep" and shutil.which("semgrep"):
        findings, files_scanned = run_semgrep(workspace, diagnostics)
    elif args.tool == "bandit" and shutil.which("bandit"):
        findings, files_scanned = run_bandit(workspace, diagnostics)
    elif args.tool in {"dependency", "dependencies", "dependency-audit"}:
        findings, files_scanned = run_dependency_scan(workspace, diagnostics)
    elif args.tool in {"secret", "secrets", "secret-scan"}:
        findings, files_scanned = run_secret_scan(workspace, diagnostics)
    else:
        if args.tool == "semgrep":
            diagnostics.append({"level": "warn", "message": "semgrep is not installed; used built-in defensive fallback scanner"})
        elif args.tool == "bandit":
            diagnostics.append({"level": "warn", "message": "bandit is not installed; used built-in defensive fallback scanner"})
        elif args.tool and args.tool not in {"fallback", "builtin"}:
            diagnostics.append({"level": "warn", "message": f"unknown security tool {args.tool!r}; used built-in defensive fallback scanner"})
        findings, files_scanned = run_fallback_scan(workspace)

    for diagnostic in diagnostics:
        record({"type": "diagnostic", "run_id": run_id, **diagnostic})
    for finding in findings:
        record({"type": "finding", "run_id": run_id, **finding})

    status = "passed" if not findings else "failed"
    score = 1.0 if not findings else 0.0
    result = finish_event(run_id, status, score, started, findings, files_scanned, diagnostics)
    record(result)
    write_output(output, events)
    return 0 if status == "passed" else 1


def run_semgrep(workspace: Path, diagnostics: list[dict]) -> tuple[list[dict], int]:
    command = ["semgrep", "--config", "auto", "--json", "--quiet", str(workspace)]
    try:
        completed = subprocess.run(command, check=False, capture_output=True, text=True, timeout=300)
    except Exception as exc:
        diagnostics.append({"level": "warn", "message": f"semgrep failed to start; used fallback scanner: {exc}"})
        return run_fallback_scan(workspace)
    if completed.returncode not in (0, 1):
        diagnostics.append({"level": "warn", "message": f"semgrep exited with {completed.returncode}; used fallback scanner"})
        return run_fallback_scan(workspace)
    try:
        payload = json.loads(completed.stdout or "{}")
    except json.JSONDecodeError:
        diagnostics.append({"level": "warn", "message": "semgrep emitted invalid JSON; used fallback scanner"})
        return run_fallback_scan(workspace)
    findings = []
    for item in payload.get("results", []):
        findings.append({
            "rule_id": str(item.get("check_id") or "semgrep"),
            "message": str(item.get("extra", {}).get("message") or "Semgrep finding"),
            "severity": str(item.get("extra", {}).get("severity") or "warning").lower(),
            "path": relativize(workspace, Path(item.get("path", ""))),
            "line": item.get("start", {}).get("line"),
            "source": "semgrep",
        })
    return findings, count_scanned_files(workspace)


def run_bandit(workspace: Path, diagnostics: list[dict]) -> tuple[list[dict], int]:
    command = ["bandit", "-r", str(workspace), "-f", "json", "-q"]
    try:
        completed = subprocess.run(command, check=False, capture_output=True, text=True, timeout=300)
    except Exception as exc:
        diagnostics.append({"level": "warn", "message": f"bandit failed to start; used fallback scanner: {exc}"})
        return run_fallback_scan(workspace)
    if completed.returncode not in (0, 1):
        diagnostics.append({"level": "warn", "message": f"bandit exited with {completed.returncode}; used fallback scanner"})
        return run_fallback_scan(workspace)
    try:
        payload = json.loads(completed.stdout or "{}")
    except json.JSONDecodeError:
        diagnostics.append({"level": "warn", "message": "bandit emitted invalid JSON; used fallback scanner"})
        return run_fallback_scan(workspace)
    findings = []
    for item in payload.get("results", []):
        finding = {
            "rule_id": str(item.get("test_id") or item.get("test_name") or "bandit"),
            "message": str(item.get("issue_text") or item.get("test_name") or "Bandit finding"),
            "severity": str(item.get("issue_severity") or "warning").lower(),
            "path": relativize(workspace, Path(item.get("filename", ""))),
            "line": item.get("line_number"),
            "source": "bandit",
        }
        confidence = item.get("issue_confidence")
        if confidence:
            finding["confidence"] = str(confidence).lower()
        issue_cwe = item.get("issue_cwe")
        if isinstance(issue_cwe, dict) and issue_cwe.get("id"):
            finding["cwe"] = issue_cwe.get("id")
        findings.append(finding)
    return findings, count_scanned_files(workspace, {".py"})


def run_dependency_scan(workspace: Path, diagnostics: list[dict]) -> tuple[list[dict], int]:
    manifests = list(iter_dependency_manifests(workspace))
    if not manifests:
        diagnostics.append({"level": "info", "message": "no dependency manifests found; skipped dependency audit"})
        return [], 0

    findings: list[dict] = []
    handled: set[Path] = set()
    requirements_files = [path for path in manifests if path.name == "requirements.txt"]
    if requirements_files and shutil.which("pip-audit"):
        for manifest in requirements_files:
            findings.extend(run_pip_audit(manifest, workspace, diagnostics))
            handled.add(manifest)
    elif requirements_files:
        diagnostics.append({"level": "warn", "message": "pip-audit is not installed; used dependency manifest fallback checks"})

    npm_lock_files = [path for path in manifests if path.name in {"package-lock.json", "npm-shrinkwrap.json"}]
    npm_roots = sorted({path.parent for path in npm_lock_files})
    if npm_roots and shutil.which("npm"):
        for root in npm_roots:
            findings.extend(run_npm_audit(root, workspace, diagnostics))
            handled.update(path for path in npm_lock_files if path.parent == root)
    elif npm_lock_files:
        diagnostics.append({"level": "warn", "message": "npm is not installed; used dependency manifest fallback checks"})

    fallback_manifests = [path for path in manifests if path not in handled]
    findings.extend(run_dependency_fallback_scan(workspace, fallback_manifests))
    return findings, len(manifests)


def run_secret_scan(workspace: Path, diagnostics: list[dict]) -> tuple[list[dict], int]:
    if shutil.which("gitleaks"):
        return run_gitleaks(workspace, diagnostics)
    diagnostics.append({"level": "warn", "message": "gitleaks is not installed; used built-in redacted secret fallback scanner"})
    return run_secret_fallback_scan(workspace)


def run_gitleaks(workspace: Path, diagnostics: list[dict]) -> tuple[list[dict], int]:
    report_path = None
    try:
        with tempfile.NamedTemporaryFile(prefix="benchforge-gitleaks-", suffix=".json", delete=False) as report:
            report_path = Path(report.name)
        command = [
            "gitleaks",
            "detect",
            "--source",
            str(workspace),
            "--report-format",
            "json",
            "--report-path",
            str(report_path),
            "--no-git",
            "--redact",
        ]
        completed = subprocess.run(command, check=False, capture_output=True, text=True, timeout=300)
        if completed.returncode not in (0, 1):
            diagnostics.append({"level": "warn", "message": f"gitleaks exited with {completed.returncode}; used built-in redacted secret fallback scanner"})
            return run_secret_fallback_scan(workspace)
        try:
            payload = json.loads(report_path.read_text(encoding="utf-8") or "[]")
        except (OSError, json.JSONDecodeError):
            diagnostics.append({"level": "warn", "message": "gitleaks emitted invalid JSON; used built-in redacted secret fallback scanner"})
            return run_secret_fallback_scan(workspace)
        findings = []
        if isinstance(payload, list):
            for item in payload:
                if not isinstance(item, dict):
                    continue
                file_value = item.get("File") or item.get("file") or item.get("Path") or item.get("path") or ""
                line_value = item.get("StartLine") or item.get("line") or item.get("Line")
                rule_id = str(item.get("RuleID") or item.get("ruleID") or item.get("rule") or "gitleaks")
                findings.append(secret_finding(
                    workspace,
                    Path(file_value),
                    line_value if isinstance(line_value, int) else None,
                    rule_id,
                    str(item.get("Description") or item.get("description") or "Possible secret detected"),
                    "critical",
                    "gitleaks",
                ))
        return findings, count_scanned_files(workspace)
    except Exception as exc:
        diagnostics.append({"level": "warn", "message": f"gitleaks failed to start; used built-in redacted secret fallback scanner: {exc}"})
        return run_secret_fallback_scan(workspace)
    finally:
        if report_path is not None:
            try:
                report_path.unlink(missing_ok=True)
            except OSError:
                pass


def run_secret_fallback_scan(workspace: Path) -> tuple[list[dict], int]:
    findings: list[dict] = []
    files_scanned = 0
    for path in iter_scan_files(workspace):
        files_scanned += 1
        for line_number, line in enumerate(read_lines(path), start=1):
            for rule_id, message, severity, pattern in SECRET_PATTERNS:
                if pattern.search(line):
                    findings.append(secret_finding(workspace, path, line_number, rule_id, message, severity, "secret-fallback"))
    return findings, files_scanned


def secret_finding(workspace: Path, path: Path, line: int | None, rule_id: str, message: str, severity: str, source: str) -> dict:
    return {
        "rule_id": rule_id,
        "message": message,
        "severity": severity,
        "path": relativize(workspace, workspace / path if not path.is_absolute() else path),
        "line": line,
        "source": source,
        "redacted": True,
        "fingerprint": secret_fingerprint(rule_id, path, line),
    }


def secret_fingerprint(rule_id: str, path: Path, line: int | None) -> str:
    return f"{rule_id}:{path.as_posix()}:{line or 0}"


def run_pip_audit(manifest: Path, workspace: Path, diagnostics: list[dict]) -> list[dict]:
    command = ["pip-audit", "-r", str(manifest), "-f", "json", "--progress-spinner", "off"]
    try:
        completed = subprocess.run(command, check=False, capture_output=True, text=True, timeout=300)
    except Exception as exc:
        diagnostics.append({"level": "warn", "message": f"pip-audit failed to start; used dependency manifest fallback checks: {exc}"})
        return run_dependency_fallback_scan(workspace, [manifest])
    if completed.returncode not in (0, 1):
        diagnostics.append({"level": "warn", "message": f"pip-audit exited with {completed.returncode}; used dependency manifest fallback checks"})
        return run_dependency_fallback_scan(workspace, [manifest])
    try:
        payload = json.loads(completed.stdout or "{}")
    except json.JSONDecodeError:
        diagnostics.append({"level": "warn", "message": "pip-audit emitted invalid JSON; used dependency manifest fallback checks"})
        return run_dependency_fallback_scan(workspace, [manifest])

    findings: list[dict] = []
    for item in payload.get("dependencies", []):
        package = str(item.get("name") or "").strip()
        version = str(item.get("version") or "").strip()
        for vuln in item.get("vulns", []):
            finding = {
                "rule_id": str(vuln.get("id") or "pip-audit"),
                "message": str(vuln.get("description") or f"Vulnerable Python dependency: {package}"),
                "severity": "high",
                "path": relativize(workspace, manifest),
                "line": dependency_line(manifest, package),
                "source": "pip-audit",
                "package": package,
                "installed_version": version,
            }
            fix_versions = vuln.get("fix_versions")
            if isinstance(fix_versions, list) and fix_versions:
                finding["fixed_versions"] = fix_versions
            aliases = vuln.get("aliases")
            if isinstance(aliases, list) and aliases:
                finding["aliases"] = aliases
            findings.append(finding)
    for item in payload.get("vulnerabilities", []):
        package = str(item.get("name") or item.get("package") or "").strip()
        findings.append({
            "rule_id": str(item.get("id") or item.get("vulnerability_id") or "pip-audit"),
            "message": str(item.get("description") or f"Vulnerable Python dependency: {package}"),
            "severity": "high",
            "path": relativize(workspace, manifest),
            "line": dependency_line(manifest, package),
            "source": "pip-audit",
            "package": package,
            "installed_version": str(item.get("version") or ""),
        })
    return findings


def run_npm_audit(root: Path, workspace: Path, diagnostics: list[dict]) -> list[dict]:
    command = ["npm", "audit", "--json", "--package-lock-only"]
    try:
        completed = subprocess.run(command, check=False, capture_output=True, text=True, timeout=300, cwd=root)
    except Exception as exc:
        diagnostics.append({"level": "warn", "message": f"npm audit failed to start; used dependency manifest fallback checks: {exc}"})
        return run_dependency_fallback_scan(workspace, dependency_manifests_in(root))
    if completed.returncode not in (0, 1):
        diagnostics.append({"level": "warn", "message": f"npm audit exited with {completed.returncode}; used dependency manifest fallback checks"})
        return run_dependency_fallback_scan(workspace, dependency_manifests_in(root))
    try:
        payload = json.loads(completed.stdout or "{}")
    except json.JSONDecodeError:
        diagnostics.append({"level": "warn", "message": "npm audit emitted invalid JSON; used dependency manifest fallback checks"})
        return run_dependency_fallback_scan(workspace, dependency_manifests_in(root))

    manifest = root / "package.json"
    lockfile = root / "package-lock.json"
    reported_path = manifest if manifest.exists() else lockfile
    findings = []
    vulnerabilities = payload.get("vulnerabilities")
    if isinstance(vulnerabilities, dict):
        for package, item in vulnerabilities.items():
            if not isinstance(item, dict):
                continue
            findings.append({
                "rule_id": f"npm-audit-{package}",
                "message": str(item.get("title") or f"Vulnerable npm dependency: {package}"),
                "severity": str(item.get("severity") or "warning").lower(),
                "path": relativize(workspace, reported_path),
                "line": dependency_line(reported_path, str(package)),
                "source": "npm-audit",
                "package": str(package),
                "range": str(item.get("range") or ""),
            })
    return findings


def run_dependency_fallback_scan(workspace: Path, manifests: list[Path]) -> list[dict]:
    findings: list[dict] = []
    for manifest in manifests:
        if manifest.name == "requirements.txt":
            findings.extend(scan_requirements_manifest(workspace, manifest))
        elif manifest.name == "package.json":
            findings.extend(scan_package_json_manifest(workspace, manifest))
    return findings


def scan_requirements_manifest(workspace: Path, manifest: Path) -> list[dict]:
    findings: list[dict] = []
    for line_number, line in enumerate(read_lines(manifest), start=1):
        match = re.match(r"\s*([A-Za-z0-9_.-]+)\s*==\s*([^\s;#]+)", line)
        if not match:
            continue
        package = normalize_package_name(match.group(1))
        version = match.group(2)
        rule = PYTHON_DEPENDENCY_RULES.get(package)
        if rule and version_less_than(version, rule[0]):
            findings.append(dependency_finding(workspace, manifest, line_number, "dependency-python-outdated", rule[1], "high", package, version, rule[0], "dependency-fallback"))
    return findings


def scan_package_json_manifest(workspace: Path, manifest: Path) -> list[dict]:
    try:
        payload = json.loads(manifest.read_text(encoding="utf-8"))
    except Exception:
        return []
    findings: list[dict] = []
    for section in ("dependencies", "devDependencies", "optionalDependencies"):
        dependencies = payload.get(section)
        if not isinstance(dependencies, dict):
            continue
        for raw_package, raw_version in dependencies.items():
            package = normalize_package_name(str(raw_package))
            version = clean_version(str(raw_version))
            rule = JS_DEPENDENCY_RULES.get(package)
            if rule and version and version_less_than(version, rule[0]):
                findings.append(dependency_finding(workspace, manifest, dependency_line(manifest, package), "dependency-js-outdated", rule[1], "high", package, str(raw_version), rule[0], "dependency-fallback"))
    return findings


def dependency_finding(workspace: Path, manifest: Path, line: int | None, rule_id: str, message: str, severity: str, package: str, version: str, minimum: str, source: str) -> dict:
    return {
        "rule_id": rule_id,
        "message": message,
        "severity": severity,
        "path": relativize(workspace, manifest),
        "line": line,
        "source": source,
        "package": package,
        "installed_version": version,
        "minimum_safe_version": minimum,
    }


def run_fallback_scan(workspace: Path) -> tuple[list[dict], int]:
    findings: list[dict] = []
    files_scanned = 0
    for path in iter_scan_files(workspace):
        files_scanned += 1
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        for rule_id, message, severity, needle in FALLBACK_RULES:
            for line_number, line in enumerate(text.splitlines(), start=1):
                if needle in line:
                    findings.append({
                        "rule_id": rule_id,
                        "message": message,
                        "severity": severity,
                        "path": relativize(workspace, path),
                        "line": line_number,
                        "source": "fallback",
                    })
    return findings, files_scanned


def iter_scan_files(root: Path, suffixes: set[str] = SCANNED_SUFFIXES):
    for path in root.rglob("*"):
        if any(part in IGNORED_DIRS for part in path.parts):
            continue
        if path.is_file() and (path.suffix.lower() in suffixes or path.name in SCANNED_FILE_NAMES):
            yield path


def iter_dependency_manifests(root: Path):
    for path in root.rglob("*"):
        if any(part in IGNORED_DIRS for part in path.parts):
            continue
        if path.is_file() and path.name in DEPENDENCY_MANIFEST_NAMES:
            yield path


def dependency_manifests_in(root: Path) -> list[Path]:
    return [path for path in root.iterdir() if path.is_file() and path.name in DEPENDENCY_MANIFEST_NAMES]


def count_scanned_files(root: Path, suffixes: set[str] = SCANNED_SUFFIXES) -> int:
    return sum(1 for _ in iter_scan_files(root, suffixes))


def read_lines(path: Path) -> list[str]:
    try:
        return path.read_text(encoding="utf-8", errors="ignore").splitlines()
    except OSError:
        return []


def dependency_line(manifest: Path, package: str) -> int | None:
    package = normalize_package_name(package)
    if not package:
        return None
    for line_number, line in enumerate(read_lines(manifest), start=1):
        normalized = normalize_package_name(line)
        if package in normalized:
            return line_number
    return None


def normalize_package_name(value: str) -> str:
    return value.lower().replace("_", "-")


def clean_version(value: str) -> str:
    return re.sub(r"^[^\d]+", "", value.strip())


def version_less_than(current: str, minimum: str) -> bool:
    current_parts = version_parts(clean_version(current))
    minimum_parts = version_parts(clean_version(minimum))
    if not current_parts or not minimum_parts:
        return False
    max_len = max(len(current_parts), len(minimum_parts))
    current_parts.extend([0] * (max_len - len(current_parts)))
    minimum_parts.extend([0] * (max_len - len(minimum_parts)))
    return current_parts < minimum_parts


def version_parts(value: str) -> list[int]:
    match = re.match(r"(\d+(?:\.\d+)*)", value)
    if not match:
        return []
    return [int(part) for part in match.group(1).split(".")]


def finish_event(run_id: str, status: str, score: float | None, started: float, findings: list[dict], files_scanned: int, diagnostics: list[dict]) -> dict:
    finding_count = len(findings)
    return {
        "type": "run_finished",
        "run_id": run_id,
        "status": status,
        "score": score,
        "metrics": {
            "wall_time_ms": int((time.monotonic() - started) * 1000),
            "files_scanned": files_scanned,
            "finding_count": finding_count,
        },
        "tests": {"total": 1, "passed": 1 if finding_count == 0 and status == "passed" else 0, "failed": 0 if finding_count == 0 and status == "passed" else 1},
        "artifacts": [],
        "safety": {"static_analysis_findings": findings, "diagnostics": diagnostics},
    }


def write_output(output: Path | None, events: list[dict]) -> None:
    if output is None:
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(json.dumps(event, ensure_ascii=False) for event in events) + "\n", encoding="utf-8")


def relativize(root: Path, path: Path) -> str:
    try:
        return str(path.resolve().relative_to(root))
    except Exception:
        return str(path)
