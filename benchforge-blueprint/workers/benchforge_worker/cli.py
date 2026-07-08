from __future__ import annotations

import argparse
import json
import sys
import time
import uuid
from pathlib import Path

from benchforge_worker import (
    __version__,
    aider_runner,
    evalplus_runner,
    security_runner,
    swebench_runner,
    terminal_bench_runner,
)


def emit(event: dict) -> None:
    print(json.dumps(event, ensure_ascii=False), flush=True)


def run_mock(args: argparse.Namespace) -> int:
    run_id = args.run_id or str(uuid.uuid4())
    emit({"type": "run_started", "run_id": run_id, "kind": "mock", "timestamp": time.time()})
    time.sleep(0.05)
    result = {
        "type": "run_finished",
        "run_id": run_id,
        "status": "passed",
        "score": 1.0,
        "metrics": {"wall_time_ms": 50},
        "tests": {"total": 1, "passed": 1, "failed": 0},
        "artifacts": [],
        "safety": {"dangerous_command_hits": [], "secret_leak_hits": []},
    }
    emit(result)
    if args.output:
        Path(args.output).write_text(json.dumps(result) + "\n", encoding="utf-8")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="benchforge-worker")
    parser.add_argument("--version", action="version", version=f"benchforge-worker {__version__}")
    sub = parser.add_subparsers(dest="command")

    run = sub.add_parser("run")
    run.add_argument("--kind", required=True, choices=["mock", "evalplus", "aider-polyglot", "terminal-bench", "swebench", "security"])
    run.add_argument("--dataset")
    run.add_argument("--subset")
    run.add_argument("--tool")
    run.add_argument("--target-config")
    run.add_argument("--run-config")
    run.add_argument("--benchmark-pack")
    run.add_argument("--workspace")
    run.add_argument("--output")
    run.add_argument("--import-path")
    run.add_argument("--run-id")

    args = parser.parse_args(argv)
    if args.command != "run":
        parser.print_help()
        return 1
    if args.kind == "mock":
        return run_mock(args)
    if args.kind == "security":
        return security_runner.run(args)
    if args.kind == "evalplus":
        return evalplus_runner.run(args)
    if args.kind == "aider-polyglot":
        return aider_runner.run(args)
    if args.kind == "terminal-bench":
        return terminal_bench_runner.run(args)
    if args.kind == "swebench":
        return swebench_runner.run(args)
    parser.error(f"unsupported worker kind: {args.kind}")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
