#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import selectors
import signal
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "app-scaffold"
MARKER = "Running `target/debug/benchforge`"
ANSI_RE = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")


def main() -> int:
    proc = subprocess.Popen(
        ["npm", "run", "tauri:dev"],
        cwd=APP,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
        preexec_fn=os.setsid,
    )
    assert proc.stdout is not None

    selector = selectors.DefaultSelector()
    selector.register(proc.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + 45
    launched = False

    try:
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                break
            for key, _ in selector.select(timeout=0.5):
                line = key.fileobj.readline()
                if not line:
                    continue
                print(line, end="")
                if MARKER in ANSI_RE.sub("", line):
                    launched = True
                    return 0
        return 1
    finally:
        selector.close()
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=5)
        if not launched and proc.returncode not in (0, -signal.SIGTERM, -signal.SIGKILL):
            print(f"tauri dev exited with status {proc.returncode}", file=sys.stderr)


if __name__ == "__main__":
    raise SystemExit(main())
