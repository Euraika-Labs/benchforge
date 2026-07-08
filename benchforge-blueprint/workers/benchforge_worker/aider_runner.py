from __future__ import annotations

import argparse

from benchforge_worker import harness_runner


def run(args: argparse.Namespace) -> int:
    return harness_runner.run(args, "aider-polyglot")
