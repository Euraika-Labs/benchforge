from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any, Callable

try:
    import resource
except ImportError:  # pragma: no cover - resource is Unix-only.
    resource = None  # type: ignore[assignment]

BASE_ENV_KEYS = {
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "TMP",
    "TEMP",
    "LANG",
    "LC_ALL",
    "SSL_CERT_FILE",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "SYSTEMROOT",
    "COMSPEC",
    "PATHEXT",
    "WINDIR",
}
SECRET_ENV_NAME_RE = re.compile(
    r"(?i)(^|_)(api[_-]?key|authorization|bearer|token|secret|password|private[_-]?key|client[_-]?secret)($|_)"
)
ENV_NAME_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
SECRET_VALUE_PATTERNS = [
    (re.compile(r"\bsk-[A-Za-z0-9][A-Za-z0-9_-]{20,}\b"), "<redacted:openai-key>"),
    (re.compile(r"\bhf_[A-Za-z0-9]{20,}\b"), "<redacted:huggingface-token>"),
    (re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{30,}\b"), "<redacted:github-token>"),
    (re.compile(r"\bA[KS]IA[0-9A-Z]{16}\b"), "<redacted:aws-access-key>"),
]
SECRET_ASSIGNMENT_RE = re.compile(
    r"(?i)([\"']?(?:api[_-]?key|secret|token|password|authorization|client[_-]?secret)[\"']?\s*[:=]\s*)([\"']?)([^\"'\s,}]{8,})([\"']?)"
)
SECRET_FLAG_RE = re.compile(
    r"(?i)((?:--?|/)(?:api[-_]?key|token|password|secret|authorization|client[-_]?secret)(?:=|\s+))(\S+)"
)
BEARER_RE = re.compile(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]{16,}")
IMPORT_FILE_SUFFIXES = {".csv", ".json", ".jsonl", ".log", ".out", ".txt", ".xml"}
TEXT_IMPORT_SUFFIXES = {".log", ".out", ".txt"}
MAX_IMPORT_BYTES = 3_000_000
MAX_IMPORT_FILES = 50


class HarnessConfigError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


def run(args: argparse.Namespace, kind: str) -> int:
    run_id = args.run_id or str(uuid.uuid4())
    workspace = Path(args.workspace or ".").resolve()
    output = Path(args.output).resolve() if args.output else None
    output_dir = output.parent if output else workspace
    raw_output_path = output_dir / f"{kind}-raw-output.txt"
    events: list[dict[str, Any]] = []
    started = time.monotonic()

    def record(event: dict[str, Any]) -> None:
        events.append(event)
        emit(event)

    record(
        {
            "type": "run_started",
            "run_id": run_id,
            "kind": kind,
            "dataset": args.dataset,
            "subset": args.subset,
            "workspace": str(workspace),
            "timestamp": time.time(),
        }
    )

    target_config = load_json_file(args.target_config)
    run_config = load_json_file(args.run_config)
    diagnostics: list[dict[str, str]] = []
    harness_config = harness_settings(target_config, kind)

    try:
        imported = imported_result_path(args, kind, target_config, harness_config, run_config, workspace, output_dir)
        if imported is not None:
            return run_imported_result(
                args,
                kind,
                run_id,
                workspace,
                output,
                output_dir,
                raw_output_path,
                started,
                events,
                diagnostics,
                record,
                imported,
            )
    except HarnessConfigError as exc:
        diagnostics.append({"level": "error", "message": exc.message})
        for diagnostic in diagnostics:
            record({"type": "diagnostic", "run_id": run_id, **diagnostic})
        result = finish_event(
            run_id,
            "error",
            None,
            started,
            {"total": None, "passed": None, "failed": None, "raw": ""},
            diagnostics,
            exc.code,
            exc.message,
            None,
        )
        record(result)
        write_output(output, events)
        return 2

    try:
        command, timeout_seconds = resolve_command(args, kind, target_config, run_config, workspace, output_dir)
        process_env = merged_env(target_config)
    except HarnessConfigError as exc:
        diagnostics.append({"level": "error", "message": exc.message})
        for diagnostic in diagnostics:
            record({"type": "diagnostic", "run_id": run_id, **diagnostic})
        result = finish_event(
            run_id,
            "error",
            None,
            started,
            {"total": None, "passed": None, "failed": None, "raw": ""},
            diagnostics,
            exc.code,
            exc.message,
            None,
        )
        record(result)
        write_output(output, events)
        return 2

    record(
        {
            "type": "diagnostic",
            "run_id": run_id,
            "level": "info",
            "message": redact_secrets(f"running {' '.join(shlex.quote(part) for part in command)}", process_env),
        }
    )

    try:
        completed = subprocess.run(
            command,
            cwd=workspace,
            env=process_env,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
        )
        peak_rss_mb = child_peak_rss_mb()
        raw_output = redact_secrets(format_raw_output(completed.stdout, completed.stderr), process_env)
        raw_output_path.parent.mkdir(parents=True, exist_ok=True)
        raw_output_path.write_text(raw_output, encoding="utf-8")
    except subprocess.TimeoutExpired as exc:
        peak_rss_mb = child_peak_rss_mb()
        raw_output = redact_secrets(format_raw_output(exc.stdout or "", exc.stderr or ""), process_env)
        raw_output_path.parent.mkdir(parents=True, exist_ok=True)
        raw_output_path.write_text(raw_output, encoding="utf-8")
        diagnostics.append({"level": "error", "message": f"{kind} harness timed out after {timeout_seconds} seconds"})
        for diagnostic in diagnostics:
            record({"type": "diagnostic", "run_id": run_id, **diagnostic})
        result = finish_event(
            run_id,
            "timeout",
            0.0,
            started,
            {"total": None, "passed": None, "failed": None, "raw": raw_output},
            diagnostics,
            "timeout",
            f"{kind} harness timed out",
            raw_output_path,
            commands_observed_count=1,
            peak_rss_mb=peak_rss_mb,
        )
        record(result)
        write_output(output, events)
        return 124
    except OSError as exc:
        diagnostics.append({"level": "error", "message": f"failed to start {kind} harness: {exc}"})
        for diagnostic in diagnostics:
            record({"type": "diagnostic", "run_id": run_id, **diagnostic})
        result = finish_event(
            run_id,
            "error",
            None,
            started,
            {"total": None, "passed": None, "failed": None, "raw": ""},
            diagnostics,
            "harness_start_failed",
            str(exc),
            None,
        )
        record(result)
        write_output(output, events)
        return 2

    summary = parse_harness_summary(raw_output)
    score = summary.get("score")
    if score is None:
        score = score_from_counts(summary)

    status = status_from_result(completed.returncode, summary)
    error_code = None
    error_message = None
    if status == "failed":
        error_code = "benchmark_failed"
        error_message = f"{kind} completed with benchmark failures"
    elif status == "error":
        error_code = "harness_failed"
        error_message = f"{kind} harness exited with {completed.returncode}"

    result = finish_event(
        run_id,
        status,
        score,
        started,
        summary,
        diagnostics,
        error_code,
        error_message,
        raw_output_path,
        completed.returncode,
        commands_observed_count=1,
        peak_rss_mb=peak_rss_mb,
    )
    record(result)
    write_output(output, events)
    return 0 if status == "passed" else 1 if status == "failed" else 2


def emit(event: dict[str, Any]) -> None:
    print(json.dumps(event, ensure_ascii=False), flush=True)


def child_peak_rss_mb() -> float | None:
    if resource is None:
        return None
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    peak = float(getattr(usage, "ru_maxrss", 0) or 0)
    if peak <= 0:
        return None
    if sys.platform == "darwin":
        return peak / (1024 * 1024)
    return peak / 1024


def load_json_file(path: str | None) -> dict[str, Any]:
    if not path:
        return {}
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except Exception:
        return {}


def first_string(*values: Any) -> str:
    for value in values:
        if isinstance(value, str):
            return value
    return ""


def resolve_command(
    args: argparse.Namespace,
    kind: str,
    target_config: dict[str, Any],
    run_config: dict[str, Any],
    workspace: Path,
    output_dir: Path,
) -> tuple[list[str], int]:
    harness_config = harness_settings(target_config, kind)
    context = worker_context(args, harness_config, target_config, workspace, output_dir)
    command_value = harness_config.get("command") or target_config.get("harness_command")
    if command_value:
        command = normalize_command(command_value, context)
    else:
        command = default_command(args, kind, harness_config, context)

    if not command:
        raise HarnessConfigError(
            "configuration_missing",
            f"{kind} requires target config harness.command, or the runner-specific fields documented for that harness",
        )

    executable = command[0]
    if not command_exists(executable):
        raise HarnessConfigError(
            "tool_missing",
            f"{kind} harness command not found: {executable}",
        )
    timeout_seconds = int(harness_config.get("timeout_seconds") or target_config.get("timeout_seconds") or 3600)
    return command, max(1, timeout_seconds)


def worker_context(
    args: argparse.Namespace,
    harness_config: dict[str, Any],
    target_config: dict[str, Any],
    workspace: Path,
    output_dir: Path,
) -> dict[str, str]:
    return {
        "dataset": args.dataset or "",
        "subset": args.subset or "",
        "workspace": str(workspace),
        "run_id": args.run_id or "",
        "benchmark_pack": args.benchmark_pack or "",
        "target_config": args.target_config or "",
        "run_config": args.run_config or "",
        "output_dir": str(output_dir),
        "model": first_string(harness_config.get("model"), target_config.get("model")),
        "base_url": first_string(harness_config.get("base_url"), target_config.get("base_url")),
    }


def imported_result_path(
    args: argparse.Namespace,
    kind: str,
    target_config: dict[str, Any],
    harness_config: dict[str, Any],
    run_config: dict[str, Any],
    workspace: Path,
    output_dir: Path,
) -> Path | None:
    raw_path = first_string(
        getattr(args, "import_path", None),
        harness_config.get("import_path"),
        harness_config.get("importPath"),
        harness_config.get("results_path"),
        harness_config.get("result_path"),
        target_config.get("import_path"),
        run_config.get("import_path"),
    )
    if not raw_path.strip():
        return None
    context = worker_context(args, harness_config, target_config, workspace, output_dir)
    formatted = format_arg(raw_path.strip(), context)
    candidate = Path(formatted)
    if not candidate.is_absolute():
        candidate = workspace / candidate
    candidate = candidate.resolve()
    allowed_roots = [workspace.resolve(), output_dir.resolve()]
    if not any(path_is_relative_to(candidate, root) for root in allowed_roots):
        raise HarnessConfigError(
            "configuration_invalid",
            f"{kind} import_path must resolve inside the worker workspace or output directory",
        )
    if not candidate.exists():
        raise HarnessConfigError("import_missing", f"{kind} import_path does not exist: {candidate}")
    if not candidate.is_file() and not candidate.is_dir():
        raise HarnessConfigError("import_invalid", f"{kind} import_path must be a file or directory: {candidate}")
    return candidate


def run_imported_result(
    args: argparse.Namespace,
    kind: str,
    run_id: str,
    workspace: Path,
    output: Path | None,
    output_dir: Path,
    raw_output_path: Path,
    started: float,
    events: list[dict[str, Any]],
    diagnostics: list[dict[str, str]],
    record: Callable[[dict[str, Any]], None],
    import_path: Path,
) -> int:
    raw_output, metadata = read_imported_output(import_path)
    raw_output = redact_secrets(raw_output)
    raw_output_path.parent.mkdir(parents=True, exist_ok=True)
    raw_output_path.write_text(
        f"--- imported from {relative_or_absolute(import_path, workspace, output_dir)} ---\n{raw_output}",
        encoding="utf-8",
    )
    diagnostics.append(
        {
            "level": "info",
            "message": f"imported existing {kind} benchmark result from {relative_or_absolute(import_path, workspace, output_dir)}",
        }
    )
    if metadata["truncated"]:
        diagnostics.append(
            {
                "level": "warn",
                "message": (
                    f"imported {kind} result was truncated to {MAX_IMPORT_BYTES} bytes; "
                    f"read {metadata['read_file_count']} of {metadata['total_file_count']} supported file(s)"
                ),
            }
        )
    for diagnostic in diagnostics:
        record({"type": "diagnostic", "run_id": run_id, **diagnostic})

    summary = parse_harness_summary(raw_output)
    score = summary.get("score")
    if score is None:
        score = score_from_counts(summary)
    status = status_from_import_summary(summary, score)
    error_code = None
    error_message = None
    if imported_summary_is_partial(summary, metadata):
        status = "error"
        score = None
        error_code = "result_import_truncated"
        error_message = (
            f"{kind} imported result was truncated or omitted supported files before a complete aggregate summary was found; "
            "provide a complete JSON, CSV, or JUnit summary file before using it as benchmark evidence"
        )
        diagnostics.append({"level": "error", "message": error_message})
    elif status == "failed":
        error_code = "benchmark_failed"
        error_message = f"{kind} imported benchmark result contains failures"
    elif status == "error":
        error_code = "result_import_unparsed"
        error_message = f"{kind} imported result did not contain a recognizable score or test summary"
    result = finish_event(
        run_id,
        status,
        score,
        started,
        summary,
        diagnostics,
        error_code,
        error_message,
        raw_output_path,
        None,
    )
    result["imported"] = True
    result["import_path"] = relative_or_absolute(import_path, workspace, output_dir)
    result["import_format"] = metadata["format"]
    result["import_formats"] = metadata["formats"]
    result["import_source"] = metadata["source"]
    result["import_read_files"] = metadata["files"]
    result["import_hash_algorithm"] = metadata["hash_algorithm"]
    result["import_file_details"] = metadata["file_details"]
    result["import_files"] = metadata["file_count"]
    result["import_total_files"] = metadata["total_file_count"]
    result["import_omitted_files"] = metadata["omitted_file_count"]
    result["import_truncated"] = metadata["truncated"]
    result["metrics"]["imported"] = 1
    result["metrics"]["import_file_count"] = metadata["file_count"]
    result["metrics"]["import_total_file_count"] = metadata["total_file_count"]
    result["metrics"]["import_omitted_file_count"] = metadata["omitted_file_count"]
    result["metrics"]["import_truncated"] = 1 if metadata["truncated"] else 0
    result["metrics"]["import_truncated_bytes"] = metadata["truncated_bytes"]
    result["metrics"]["import_format"] = metadata["format"]
    result["metrics"]["import_source"] = metadata["source"]
    result["metrics"]["import_path"] = result["import_path"]
    record(result)
    write_output(output, events)
    return 0 if status == "passed" else 1 if status == "failed" else 2


def read_imported_output(path: Path) -> tuple[str, dict[str, Any]]:
    if path.is_file():
        ensure_supported_import_file(path)
        text, read_detail = read_text_tail_with_metadata(path, MAX_IMPORT_BYTES)
        return text, import_metadata(
            path,
            [path],
            1,
            read_file_count=1,
            omitted_file_count=0,
            truncated_bytes=read_detail["truncated_bytes"],
            file_details=[import_file_detail(path, path, read_detail)],
        )
    files, total_file_count = imported_files_with_stats(path)
    if not files:
        raise HarnessConfigError(
            "import_invalid",
            f"import directory has no supported result files ({', '.join(sorted(IMPORT_FILE_SUFFIXES))}): {path}",
        )
    chunks = []
    remaining_bytes = MAX_IMPORT_BYTES
    read_files = []
    file_details = []
    truncated_bytes = 0
    for item in files:
        if remaining_bytes <= 0:
            break
        text, read_detail = read_text_tail_with_metadata(item, remaining_bytes)
        chunks.append(f"--- file: {item.relative_to(path)} ---\n{text}")
        read_files.append(item)
        file_details.append(import_file_detail(path, item, read_detail))
        remaining_bytes -= int(read_detail["read_bytes"])
        truncated_bytes += int(read_detail["truncated_bytes"])
    omitted_file_count = max(0, total_file_count - len(read_files))
    if omitted_file_count:
        chunks.append(f"[omitted {omitted_file_count} supported result file(s) after import limits]")
    return "\n".join(chunks), import_metadata(
        path,
        read_files,
        total_file_count,
        read_file_count=len(read_files),
        omitted_file_count=omitted_file_count,
        truncated_bytes=truncated_bytes,
        file_details=file_details,
    )


def ensure_supported_import_file(path: Path) -> None:
    if path.suffix.lower() in IMPORT_FILE_SUFFIXES:
        return
    supported = ", ".join(sorted(IMPORT_FILE_SUFFIXES))
    raise HarnessConfigError(
        "import_invalid",
        f"unsupported result file type for import_path: {path.name}; supported suffixes: {supported}",
    )


def import_metadata(
    path: Path,
    files: list[Path],
    total_file_count: int,
    *,
    read_file_count: int,
    omitted_file_count: int,
    truncated_bytes: int,
    file_details: list[dict[str, Any]],
) -> dict[str, Any]:
    formats = sorted({import_format_for_path(item) for item in files})
    read_files = [
        str(item.relative_to(path)) if path.is_dir() else item.name
        for item in files
    ]
    return {
        "source": "directory" if path.is_dir() else "file",
        "files": read_files,
        "file_count": read_file_count,
        "read_file_count": read_file_count,
        "total_file_count": total_file_count,
        "omitted_file_count": omitted_file_count,
        "truncated": truncated_bytes > 0 or omitted_file_count > 0,
        "truncated_bytes": truncated_bytes,
        "format": formats[0] if len(formats) == 1 else "mixed",
        "formats": formats,
        "hash_algorithm": "sha256",
        "file_details": file_details,
    }


def import_file_detail(root: Path, path: Path, read_detail: dict[str, Any]) -> dict[str, Any]:
    item_path = str(path.relative_to(root)) if root.is_dir() else path.name
    return {
        "path": item_path,
        "format": import_format_for_path(path),
        "size_bytes": read_detail["size_bytes"],
        "read_bytes": read_detail["read_bytes"],
        "truncated_bytes": read_detail["truncated_bytes"],
        "read_sha256": read_detail["read_sha256"],
        "sha256": read_detail["sha256"],
    }


def imported_files(path: Path) -> list[Path]:
    files, _ = imported_files_with_stats(path)
    return files


def imported_files_with_stats(path: Path) -> tuple[list[Path], int]:
    if path.is_file():
        return [path], 1
    root = path.resolve()
    files: list[Path] = []
    total_file_count = 0
    for item in sorted(path.rglob("*"), key=lambda item: import_file_sort_key(path, item)):
        if item.suffix.lower() not in IMPORT_FILE_SUFFIXES:
            continue
        if item.is_symlink():
            raise HarnessConfigError(
                "import_invalid",
                f"import directory contains a symlinked result file, which is not allowed: {item.relative_to(path)}",
            )
        if not item.is_file():
            continue
        resolved = item.resolve()
        if not path_is_relative_to(resolved, root):
            raise HarnessConfigError(
                "import_invalid",
                f"import directory result file resolves outside the import directory: {item.relative_to(path)}",
            )
        total_file_count += 1
        if len(files) < MAX_IMPORT_FILES:
            files.append(item)
    return files, total_file_count


def import_file_sort_key(root: Path, path: Path) -> tuple[int, str]:
    suffix = path.suffix.lower()
    priority = 0 if suffix in {".json", ".jsonl", ".xml", ".csv"} else 1
    try:
        relative = str(path.relative_to(root))
    except ValueError:
        relative = str(path)
    return priority, relative


def import_format_for_path(path: Path) -> str:
    suffix = path.suffix.lower()
    if suffix in TEXT_IMPORT_SUFFIXES:
        return "text"
    return suffix.removeprefix(".") or "unknown"


def read_text_tail(path: Path) -> str:
    text, _ = read_text_tail_with_metadata(path, MAX_IMPORT_BYTES)
    return text


def read_text_tail_with_metadata(path: Path, max_bytes: int) -> tuple[str, dict[str, Any]]:
    size = path.stat().st_size
    with path.open("rb") as handle:
        if size > max_bytes:
            handle.seek(size - max_bytes)
            truncated_bytes = size - max_bytes
            prefix = f"[truncated first {truncated_bytes} bytes]\n"
        else:
            truncated_bytes = 0
            prefix = ""
        data = handle.read(max_bytes)
    read_sha256 = hashlib.sha256(data).hexdigest()
    return prefix + data.decode("utf-8", errors="replace"), {
        "size_bytes": size,
        "read_bytes": len(data),
        "truncated_bytes": truncated_bytes,
        "read_sha256": read_sha256,
        "sha256": read_sha256 if truncated_bytes == 0 else None,
    }


def status_from_import_summary(summary: dict[str, Any], score: float | None) -> str:
    failed = summary.get("failed")
    total = summary.get("total")
    if isinstance(failed, int) and failed > 0:
        return "failed"
    if isinstance(failed, int) and failed == 0 and isinstance(total, int) and total > 0:
        return "passed"
    if isinstance(score, (int, float)):
        return "passed" if float(score) >= 1.0 else "failed"
    return "error"


def imported_summary_is_partial(summary: dict[str, Any], metadata: dict[str, Any]) -> bool:
    if not metadata.get("truncated"):
        return False
    return summary.get("summary_source") in {"text", "json_items"}


def path_is_relative_to(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
        return True
    except ValueError:
        return False


def relative_or_absolute(path: Path, *roots: Path) -> str:
    for root in roots:
        try:
            return str(path.relative_to(root.resolve()))
        except ValueError:
            continue
    return str(path)


def harness_settings(target_config: dict[str, Any], kind: str) -> dict[str, Any]:
    settings = target_config.get("harness")
    if isinstance(settings, dict):
        return settings
    normalized = kind.replace("-", "_")
    settings = target_config.get(normalized)
    return settings if isinstance(settings, dict) else {}


def normalize_command(value: Any, context: dict[str, str]) -> list[str]:
    if isinstance(value, str):
        return [format_arg(part, context) for part in shlex.split(value)]
    if isinstance(value, list):
        return [format_arg(str(part), context) for part in value]
    raise HarnessConfigError("configuration_invalid", "harness.command must be a string or list")


def format_arg(value: str, context: dict[str, str]) -> str:
    try:
        return value.format(**context)
    except KeyError as exc:
        raise HarnessConfigError("configuration_invalid", f"unknown harness.command placeholder: {exc}") from exc


def default_command(args: argparse.Namespace, kind: str, harness_config: dict[str, Any], context: dict[str, str]) -> list[str] | None:
    if kind == "evalplus":
        samples = harness_config.get("samples") or harness_config.get("samples_path")
        if not samples:
            return None
        tool = args.tool or harness_config.get("tool") or "evalplus.evaluate"
        dataset = args.dataset or harness_config.get("dataset") or "humaneval"
        return [str(tool), "--dataset", str(dataset), "--samples", format_arg(str(samples), context)]
    return None


def command_exists(executable: str) -> bool:
    path = Path(executable)
    return path.exists() or shutil.which(executable) is not None


def merged_env(target_config: dict[str, Any]) -> dict[str, str]:
    env = {
        key: value
        for key, value in os.environ.items()
        if key.upper() in BASE_ENV_KEYS and isinstance(value, str)
    }
    env.setdefault("PATH", os.defpath)
    harness = target_config.get("harness")
    harness = harness if isinstance(harness, dict) else {}

    for key in env_passthrough_names(harness.get("env_passthrough", target_config.get("env_passthrough"))):
        if key in os.environ:
            env[key] = os.environ[key]

    configured_env = harness.get("env")
    if isinstance(configured_env, dict):
        for key, value in configured_env.items():
            if not isinstance(key, str) or not ENV_NAME_RE.fullmatch(key):
                raise HarnessConfigError("configuration_invalid", "harness.env keys must be valid environment variable names")
            if not isinstance(value, str):
                raise HarnessConfigError("configuration_invalid", f"harness.env.{key} must be a string")
            if secret_env_name(key):
                raise HarnessConfigError(
                    "configuration_secret_env",
                    f"harness.env.{key} looks like a secret; use harness.env_passthrough to pass a named host variable",
                )
            env[key] = value
    return env


def env_passthrough_names(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        raw_names = re.split(r"[\s,]+", value.strip()) if value.strip() else []
    elif isinstance(value, list):
        raw_names = value
    else:
        raise HarnessConfigError("configuration_invalid", "harness.env_passthrough must be a string or list")

    names: list[str] = []
    for raw in raw_names:
        if not isinstance(raw, str) or not raw.strip():
            raise HarnessConfigError("configuration_invalid", "harness.env_passthrough entries must be non-empty strings")
        name = raw.strip()
        if not ENV_NAME_RE.fullmatch(name):
            raise HarnessConfigError(
                "configuration_invalid",
                f"harness.env_passthrough entry is not a valid environment variable name: {name}",
            )
        if name not in names:
            names.append(name)
    return names


def secret_env_name(name: str) -> bool:
    return SECRET_ENV_NAME_RE.search(name) is not None


def redact_secrets(text: str, env: dict[str, str] | None = None) -> str:
    if not text:
        return text
    redacted = text
    for value in secret_values(env):
        redacted = redacted.replace(value, "<redacted:secret>")
    for pattern, replacement in SECRET_VALUE_PATTERNS:
        redacted = pattern.sub(replacement, redacted)
    redacted = BEARER_RE.sub(r"\1<redacted:bearer-token>", redacted)
    redacted = SECRET_ASSIGNMENT_RE.sub(
        lambda match: f"{match.group(1)}{match.group(2)}<redacted:secret>{match.group(4)}",
        redacted,
    )
    redacted = SECRET_FLAG_RE.sub(r"\1<redacted:secret>", redacted)
    return redacted


def secret_values(env: dict[str, str] | None = None) -> list[str]:
    sources = [os.environ]
    if env is not None:
        sources.append(env)
    values: set[str] = set()
    for source in sources:
        for key, value in source.items():
            if secret_env_name(key) and isinstance(value, str) and len(value) >= 8:
                values.add(value)
    return sorted(values, key=len, reverse=True)


def format_raw_output(stdout: str | bytes | None, stderr: str | bytes | None) -> str:
    stdout_text = decode_text(stdout)
    stderr_text = decode_text(stderr)
    return f"--- stdout ---\n{stdout_text}\n--- stderr ---\n{stderr_text}\n"


def decode_text(value: str | bytes | None) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return value


def parse_harness_summary(raw_output: str) -> dict[str, Any]:
    candidates = json_candidates(raw_output)
    for candidate in reversed(candidates):
        summary = summary_from_json(candidate)
        if summary:
            summary["raw"] = raw_output[-4000:]
            summary["summary_source"] = "json"
            return summary
    aggregate = summary_from_result_items(candidates)
    if aggregate:
        aggregate["raw"] = raw_output[-4000:]
        aggregate["summary_source"] = "json_items"
        return aggregate
    csv_summary = summary_from_csv(raw_output)
    if csv_summary:
        csv_summary["raw"] = raw_output[-4000:]
        csv_summary["summary_source"] = "csv"
        return csv_summary
    xml_summary = summary_from_junit_xml(raw_output)
    if xml_summary:
        xml_summary["raw"] = raw_output[-4000:]
        xml_summary["summary_source"] = "junit_xml"
        return xml_summary
    summary = summary_from_text(raw_output)
    summary["raw"] = raw_output[-4000:]
    summary["summary_source"] = "text"
    return summary


def json_candidates(text: str) -> list[Any]:
    candidates: list[Any] = []
    stripped = text.strip()
    if stripped:
        try:
            candidates.append(json.loads(stripped))
        except json.JSONDecodeError:
            pass
    for line in text.splitlines():
        line = line.strip()
        if not line or line[0] not in "[{":
            continue
        try:
            candidates.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return candidates


def summary_from_json(value: Any) -> dict[str, Any] | None:
    if isinstance(value, list):
        return summary_from_result_items(value)
    if not isinstance(value, dict):
        return None

    direct = summary_from_count_mapping(value)
    if direct and summary_has_counts(direct):
        return direct

    score = direct.get("score") if direct else None
    for key in ["tests", "metrics", "summary", "stats", "counts", "report", "evaluation"]:
        nested = value.get(key)
        if not isinstance(nested, dict):
            continue
        nested_summary = summary_from_count_mapping(nested)
        if nested_summary and nested_summary.get("score") is not None and score is None:
            score = nested_summary.get("score")
        if nested_summary and summary_has_counts(nested_summary):
            return merge_summary_score(nested_summary, score)

    for key in ["results", "instances", "tasks", "cases", "items", "examples", "records"]:
        nested = value.get(key)
        nested_summary = summary_from_collection(nested)
        if nested_summary:
            return merge_summary_score(nested_summary, score)

    return direct


def summary_from_count_mapping(source: dict[str, Any]) -> dict[str, Any] | None:
    total = first_count(source, ["total", "num_tests", "n", "total_tests", "total_instances", "n_total"])
    passed = first_count(
        source,
        [
            "passed",
            "successes",
            "success",
            "resolved",
            "passed_tests",
            "resolved_count",
            "num_resolved",
            "n_passed",
            "correct",
            "succeeded",
        ],
    )
    failed = first_count(
        source,
        [
            "failed",
            "failures",
            "failure",
            "unresolved",
            "failed_tests",
            "unresolved_count",
            "num_unresolved",
            "n_failed",
            "incorrect",
            "errors",
        ],
    )
    score = first_number(source, ["score", "pass_rate", "accuracy", "pass@1", "pass_at_1", "resolved_rate"])
    percent_score = first_number(source, ["score_percent", "pass_percent", "accuracy_percent", "pass@1_percent"])
    if score is None and percent_score is not None:
        score = percent_score / 100.0
    if total is None and passed is not None and failed is not None:
        total = passed + failed
    if failed is None and total is not None and passed is not None:
        failed = max(total - passed, 0)
    if passed is None and total is not None and failed is not None:
        passed = max(total - failed, 0)
    if total is None and score is None and (passed is None or failed is None):
        return None
    return {
        "total": int(total) if total is not None else None,
        "passed": int(passed) if passed is not None else None,
        "failed": int(failed) if failed is not None else None,
        "score": score,
    }


def summary_from_collection(value: Any) -> dict[str, Any] | None:
    if isinstance(value, list):
        return summary_from_result_items(value)
    if isinstance(value, dict):
        direct = summary_from_count_mapping(value)
        if direct and summary_has_counts(direct):
            return direct
        return summary_from_result_items(list(value.values()))
    return None


def summary_from_result_items(items: list[Any]) -> dict[str, Any] | None:
    results = [item_result(item) for item in items]
    known = [result for result in results if result is not None]
    total = len(known)
    if not total:
        return None
    passed = sum(1 for result in known if result)
    failed = total - passed
    return {"total": total, "passed": passed, "failed": failed, "score": passed / total}


def summary_has_counts(summary: dict[str, Any]) -> bool:
    return isinstance(summary.get("total"), int) or (
        isinstance(summary.get("passed"), int) and isinstance(summary.get("failed"), int)
    )


def merge_summary_score(summary: dict[str, Any], score: float | None) -> dict[str, Any]:
    if score is not None:
        summary = {**summary, "score": score}
    return summary


def summary_from_csv(text: str) -> dict[str, Any] | None:
    summaries = [
        summary
        for block in delimited_import_blocks(text)
        if (summary := summary_from_csv_block(block)) is not None
    ]
    if not summaries:
        return None
    if len(summaries) == 1:
        return summaries[0]
    counted = [
        summary
        for summary in summaries
        if isinstance(summary.get("total"), int)
        and isinstance(summary.get("passed"), int)
        and isinstance(summary.get("failed"), int)
    ]
    if not counted:
        return summaries[-1]
    total = sum(int(summary["total"]) for summary in counted)
    passed = sum(int(summary["passed"]) for summary in counted)
    failed = sum(int(summary["failed"]) for summary in counted)
    return {
        "total": total,
        "passed": passed,
        "failed": failed,
        "score": passed / total if total else None,
    }


def delimited_import_blocks(text: str) -> list[str]:
    blocks: list[str] = []
    current: list[str] = []
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("--- file: ") and stripped.endswith("---"):
            if current:
                blocks.append("\n".join(current))
                current = []
            continue
        current.append(line)
    if current:
        blocks.append("\n".join(current))
    return blocks


def summary_from_csv_block(text: str) -> dict[str, Any] | None:
    stripped = text.strip()
    if not stripped:
        return None
    first_line = stripped.splitlines()[0]
    if "," not in first_line:
        return None
    try:
        reader = csv.DictReader(io.StringIO(stripped))
        if not reader.fieldnames:
            return None
        rows = [
            normalized_csv_row(row)
            for row in reader
            if any(isinstance(cell, str) and cell.strip() for cell in row.values())
        ]
    except csv.Error:
        return None
    if not rows:
        return None
    header_keys = {normalize_summary_key(key) for key in reader.fieldnames if key}
    known_keys = {
        "accuracy",
        "accuracy_percent",
        "correct",
        "failed",
        "failed_tests",
        "failure",
        "incorrect",
        "n_failed",
        "n_passed",
        "n_total",
        "num_resolved",
        "num_tests",
        "num_unresolved",
        "ok",
        "outcome",
        "pass",
        "pass_1",
        "pass_at_1",
        "pass_percent",
        "pass_rate",
        "passed",
        "passed_tests",
        "resolved",
        "resolved_count",
        "resolved_rate",
        "result",
        "score",
        "score_percent",
        "status",
        "success",
        "successes",
        "total",
        "total_instances",
        "total_tests",
        "unresolved",
        "unresolved_count",
        "verdict",
    }
    if not header_keys.intersection(known_keys):
        return None
    for row in rows:
        direct = summary_from_count_mapping(row)
        if direct and summary_has_counts(direct):
            return direct
    return summary_from_result_items(rows)


def normalized_csv_row(row: dict[str | None, str | None]) -> dict[str, Any]:
    normalized: dict[str, Any] = {}
    for key, value in row.items():
        if key is None:
            continue
        normalized[normalize_summary_key(key)] = normalize_csv_value(value)
    return normalized


def normalize_summary_key(key: str) -> str:
    normalized = key.strip().lower()
    normalized = re.sub(r"[\s./-]+", "_", normalized)
    return normalized.replace("@", "_at_")


def normalize_csv_value(value: str | None) -> Any:
    if value is None:
        return ""
    stripped = value.strip()
    lowered = stripped.lower()
    if lowered in {"true", "yes", "y"}:
        return True
    if lowered in {"false", "no", "n"}:
        return False
    try:
        if re.fullmatch(r"[+-]?\d+", stripped):
            return int(stripped)
        if re.fullmatch(r"[+-]?(?:\d+\.\d*|\.\d+)(?:e[+-]?\d+)?", stripped, flags=re.IGNORECASE):
            return float(stripped)
    except ValueError:
        pass
    return stripped


def summary_from_junit_xml(text: str) -> dict[str, Any] | None:
    summaries = [
        summary
        for block in delimited_import_blocks(text)
        if (summary := summary_from_junit_xml_block(block)) is not None
    ]
    if not summaries:
        return None
    total = sum(int(summary["total"]) for summary in summaries)
    passed = sum(int(summary["passed"]) for summary in summaries)
    failed = sum(int(summary["failed"]) for summary in summaries)
    return {
        "total": total,
        "passed": passed,
        "failed": failed,
        "score": passed / total if total else None,
    }


def summary_from_junit_xml_block(text: str) -> dict[str, Any] | None:
    stripped = text.strip()
    if not stripped.startswith("<"):
        return None
    try:
        root = ET.fromstring(stripped)
    except ET.ParseError:
        return None

    suite_summaries = [
        summary
        for suite in root.iter()
        if local_xml_name(suite.tag) == "testsuite"
        if (summary := summary_from_junit_testsuite(suite)) is not None
    ]
    if suite_summaries:
        total = sum(int(summary["total"]) for summary in suite_summaries)
        passed = sum(int(summary["passed"]) for summary in suite_summaries)
        failed = sum(int(summary["failed"]) for summary in suite_summaries)
        return {
            "total": total,
            "passed": passed,
            "failed": failed,
            "score": passed / total if total else None,
        }

    cases = [case for case in root.iter() if local_xml_name(case.tag) == "testcase"]
    if not cases:
        return None
    failed = sum(1 for case in cases if junit_case_failed(case))
    total = len(cases)
    passed = total - failed
    return {
        "total": total,
        "passed": passed,
        "failed": failed,
        "score": passed / total if total else None,
    }


def summary_from_junit_testsuite(suite: ET.Element) -> dict[str, Any] | None:
    total = xml_int_attr(suite, "tests")
    if total is None:
        cases = [case for case in suite if local_xml_name(case.tag) == "testcase"]
        if not cases:
            return None
        failed = sum(1 for case in cases if junit_case_failed(case))
        total = len(cases)
        passed = total - failed
    else:
        failures = xml_int_attr(suite, "failures") or 0
        errors = xml_int_attr(suite, "errors") or 0
        skipped = xml_int_attr(suite, "skipped") or 0
        failed = failures + errors + skipped
        passed = max(total - failed, 0)
    return {
        "total": int(total),
        "passed": int(passed),
        "failed": int(failed),
        "score": passed / total if total else None,
    }


def junit_case_failed(case: ET.Element) -> bool:
    return any(local_xml_name(child.tag) in {"error", "failure", "skipped"} for child in case)


def xml_int_attr(element: ET.Element, name: str) -> int | None:
    value = element.attrib.get(name)
    if value is None:
        return None
    try:
        return int(float(value))
    except ValueError:
        return None


def local_xml_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1] if "}" in tag else tag


def item_result(item: Any) -> bool | None:
    if isinstance(item, bool):
        return item
    if isinstance(item, str):
        return status_result(item)
    if not isinstance(item, dict):
        return None
    for key in ["passed", "pass", "success", "resolved", "ok", "correct"]:
        parsed = scalar_result(item.get(key))
        if parsed is not None:
            return parsed
    for key in ["failed", "failure", "unresolved", "error", "incorrect"]:
        parsed = scalar_result(item.get(key))
        if parsed is not None:
            return not parsed
    for key in ["status", "result", "outcome", "verdict"]:
        parsed = scalar_result(item.get(key))
        if parsed is not None:
            return parsed
    return None


def scalar_result(value: Any) -> bool | None:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if value == 1:
            return True
        if value == 0:
            return False
    if isinstance(value, str):
        return status_result(value)
    return None


def status_result(value: str) -> bool | None:
    normalized = value.strip().lower().replace("_", "-")
    if normalized in {"1", "true", "yes", "y", "passed", "pass", "success", "successful", "resolved", "ok", "correct"}:
        return True
    if normalized in {
        "0",
        "false",
        "no",
        "n",
        "failed",
        "fail",
        "failure",
        "unresolved",
        "error",
        "errored",
        "incorrect",
        "timeout",
        "timed-out",
        "cancelled",
        "canceled",
    }:
        return False
    return None


def first_count(source: dict[str, Any], keys: list[str]) -> float | None:
    for key in keys:
        value = source.get(key)
        if isinstance(value, bool):
            continue
        if isinstance(value, (int, float)):
            return float(value)
        if isinstance(value, list):
            return float(len(value))
        if isinstance(value, dict):
            return float(len(value))
        if isinstance(value, str):
            try:
                return float(value)
            except ValueError:
                continue
    return None


def first_number(source: dict[str, Any], keys: list[str]) -> float | None:
    for key in keys:
        value = source.get(key)
        if isinstance(value, bool):
            continue
        if isinstance(value, (int, float)):
            return float(value)
        if isinstance(value, str):
            try:
                return float(value)
            except ValueError:
                continue
    return None


def summary_from_text(text: str) -> dict[str, Any]:
    score = None
    for pattern in [r"pass@1\s*[:=]\s*([0-9.]+)", r"accuracy\s*[:=]\s*([0-9.]+)", r"score\s*[:=]\s*([0-9.]+)"]:
        match = re.search(pattern, text, flags=re.IGNORECASE)
        if match:
            score = float(match.group(1))
            break
    passed = failed = total = None
    match = re.search(r"(\d+)\s+passed(?:,\s*(\d+)\s+failed)?", text, flags=re.IGNORECASE)
    if match:
        passed = int(match.group(1))
        failed = int(match.group(2) or 0)
        total = passed + failed
    match = re.search(r"passed\s+(\d+)\s*/\s*(\d+)", text, flags=re.IGNORECASE)
    if match:
        passed = int(match.group(1))
        total = int(match.group(2))
        failed = total - passed
    return {"total": total, "passed": passed, "failed": failed, "score": score}


def score_from_counts(summary: dict[str, Any]) -> float | None:
    total = summary.get("total")
    passed = summary.get("passed")
    if isinstance(total, int) and total > 0 and isinstance(passed, int):
        return passed / total
    return None


def status_from_result(returncode: int, summary: dict[str, Any]) -> str:
    failed = summary.get("failed")
    if returncode == 0:
        return "passed"
    if isinstance(failed, int) and failed > 0:
        return "failed"
    return "error"


def finish_event(
    run_id: str,
    status: str,
    score: float | None,
    started: float,
    summary: dict[str, Any],
    diagnostics: list[dict[str, str]],
    error_code: str | None,
    error_message: str | None,
    raw_output_path: Path | None,
    exit_code: int | None = None,
    commands_observed_count: int = 0,
    peak_rss_mb: float | None = None,
) -> dict[str, Any]:
    tests = {
        "total": summary.get("total"),
        "passed": summary.get("passed"),
        "failed": summary.get("failed"),
        "raw": summary.get("raw", ""),
        "summary_source": summary.get("summary_source"),
    }
    artifacts = []
    if raw_output_path is not None:
        artifacts.append({"kind": "harness_raw_output", "path": str(raw_output_path)})
    return {
        "type": "run_finished",
        "run_id": run_id,
        "status": status,
        "score": score,
        "error_code": error_code,
        "error_message": error_message,
        "metrics": {
            "wall_time_ms": int((time.monotonic() - started) * 1000),
            "harness_exit_code": exit_code,
            "commands_observed_count": commands_observed_count,
            "peak_rss_mb": peak_rss_mb,
            "total_tests": summary.get("total"),
            "passed_tests": summary.get("passed"),
            "failed_tests": summary.get("failed"),
            "summary_source": summary.get("summary_source"),
        },
        "tests": tests,
        "artifacts": artifacts,
        "safety": {"diagnostics": diagnostics},
    }


def write_output(output: Path | None, events: list[dict[str, Any]]) -> None:
    if output is None:
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(json.dumps(event, ensure_ascii=False) for event in events) + "\n", encoding="utf-8")
