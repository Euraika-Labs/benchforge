from __future__ import annotations

import hashlib
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class HarnessRunnerTests(unittest.TestCase):
    def run_cli(self, temp_dir: Path, *args: str) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        source_path = str(Path(__file__).resolve().parents[1])
        env["PYTHONPATH"] = source_path + os.pathsep + env.get("PYTHONPATH", "")
        return subprocess.run(
            [sys.executable, "-m", "benchforge_worker.cli", *args],
            cwd=temp_dir,
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

    def run_worker(
        self,
        temp_dir: Path,
        *args: str,
        env_overrides: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        if env_overrides:
            env.update(env_overrides)
        source_path = str(Path(__file__).resolve().parents[1])
        env["PYTHONPATH"] = source_path + os.pathsep + env.get("PYTHONPATH", "")
        return subprocess.run(
            [sys.executable, "-m", "benchforge_worker.cli", "run", *args],
            cwd=temp_dir,
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_cli_version_reports_worker_version(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            completed = self.run_cli(Path(raw_dir), "--version")

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertRegex(completed.stdout.strip(), r"^benchforge-worker \d+\.\d+\.\d+$")
        self.assertEqual(completed.stderr, "")

    def write_tool(self, temp_dir: Path, name: str, body: str) -> Path:
        tool = temp_dir / name
        tool.write_text("#!/usr/bin/env python3\n" + body, encoding="utf-8")
        tool.chmod(tool.stat().st_mode | stat.S_IXUSR)
        return tool

    def symlink_or_skip(self, link: Path, target: Path) -> None:
        try:
            link.symlink_to(target)
        except OSError as exc:
            self.skipTest(f"symlinks are not available in this environment: {exc}")

    def final_event(self, output: Path) -> dict:
        events = [json.loads(line) for line in output.read_text(encoding="utf-8").splitlines()]
        self.assertGreaterEqual(len(events), 2)
        return events[-1]

    def assert_import_metadata(
        self,
        event: dict,
        *,
        import_format: str,
        import_source: str = "file",
        import_files: int = 1,
        summary_source: str,
    ) -> None:
        self.assertTrue(event["imported"])
        self.assertEqual(event["import_format"], import_format)
        if import_format == "mixed":
            self.assertGreater(len(event["import_formats"]), 1)
        else:
            self.assertIn(import_format, event["import_formats"])
        self.assertEqual(event["import_source"], import_source)
        self.assertEqual(event["import_files"], import_files)
        self.assertEqual(len(event["import_read_files"]), import_files)
        self.assertIn("import_unsupported_file_count", event)
        self.assertIn("import_unsupported_files", event)
        self.assertEqual(event["metrics"].get("import_unsupported_file_count"), event["import_unsupported_file_count"])
        self.assertEqual(event["import_hash_algorithm"], "sha256")
        self.assertEqual(len(event["import_file_details"]), import_files)
        for detail in event["import_file_details"]:
            self.assertIn(detail["path"], event["import_read_files"])
            self.assertRegex(detail["read_sha256"], r"^[0-9a-f]{64}$")
            self.assertGreaterEqual(detail["size_bytes"], detail["read_bytes"])
            self.assertGreaterEqual(detail["truncated_bytes"], 0)
        self.assertEqual(event["metrics"]["imported"], 1)
        self.assertEqual(event["metrics"]["import_format"], import_format)
        self.assertEqual(event["metrics"]["import_source"], import_source)
        self.assertEqual(event["metrics"]["import_file_count"], import_files)
        self.assertEqual(event["metrics"]["summary_source"], summary_source)
        self.assertEqual(event["metrics"]["commands_observed_count"], 0)
        self.assertEqual(event["tests"]["summary_source"], summary_source)

    def test_evalplus_bridge_runs_configured_command_and_parses_json_summary(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-evalplus",
                "import json, sys\nprint(json.dumps({'total': 4, 'passed': 3, 'failed': 1, 'score': 0.75}))\nsys.exit(1)\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--dataset",
                "humaneval",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["error_code"], "benchmark_failed")
            self.assertEqual(event["metrics"]["commands_observed_count"], 1)
            self.assertIn("peak_rss_mb", event["metrics"])
            self.assertEqual(event["tests"]["total"], 4)
            self.assertEqual(event["tests"]["passed"], 3)
            self.assertEqual(event["tests"]["failed"], 1)
            raw_artifact = Path(event["artifacts"][0]["path"])
            self.assertTrue(raw_artifact.exists())
            self.assertIn('"failed": 1', raw_artifact.read_text(encoding="utf-8"))

    def test_terminal_bridge_runs_configured_command_and_parses_text_score(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-terminal-bench",
                "print('pass@1: 0.625')\nprint('5 passed, 3 failed')\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(
                json.dumps({"harness": {"command": [str(tool)], "timeout_seconds": 5}}),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--subset",
                "small",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.625)
            self.assertEqual(event["error_code"], "benchmark_failed")
            self.assertEqual(event["tests"]["total"], 8)
            self.assertEqual(event["tests"]["passed"], 5)
            self.assertEqual(event["tests"]["failed"], 3)

    def test_external_harness_zero_exit_still_fails_when_summary_reports_failures(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-green-exit-failing-harness",
                "import json\nprint(json.dumps({'total': 4, 'passed': 3, 'failed': 1, 'score': 0.75}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["error_code"], "benchmark_failed")
            self.assertEqual(event["tests"]["failed"], 1)

    def test_external_harness_parses_json_tests_array_as_benchmark_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-tests-array-harness",
                "import json\n"
                "print(json.dumps({'tests': ["
                "{'name': 'one', 'status': 'passed'},"
                "{'name': 'two', 'status': 'failed'},"
                "{'name': 'three', 'ok': True}"
                "]}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertAlmostEqual(event["score"], 2 / 3)
            self.assertEqual(event["error_code"], "benchmark_failed")
            self.assertEqual(event["tests"]["total"], 3)
            self.assertEqual(event["tests"]["passed"], 2)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_external_harness_zero_exit_score_only_partial_result_is_failed(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-green-exit-score-harness",
                "import json\nprint(json.dumps({'score': 0.5}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "aider-polyglot",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.5)
            self.assertEqual(event["error_code"], "benchmark_failed")

    def test_external_harness_percent_score_is_normalized_before_status(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-percent-score-harness",
                "import json\nprint(json.dumps({'score': 75}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["error_code"], "benchmark_failed")

    def test_external_harness_invalid_score_without_counts_is_unparsed_error(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-invalid-score-harness",
                "import json\nprint(json.dumps({'score': 125}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertIsNone(event["score"])
            self.assertEqual(event["error_code"], "harness_unparsed")

    def test_external_harness_zero_exit_without_summary_is_error_not_pass(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-green-empty-harness",
                "print('completed without benchmark summary')\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertIsNone(event["score"])
            self.assertEqual(event["error_code"], "harness_unparsed")
            self.assertIn("recognizable score or test summary", event["error_message"])
            self.assertIsNone(event["tests"]["total"])
            self.assertIsNone(event["tests"]["passed"])
            self.assertIsNone(event["tests"]["failed"])

    def test_external_harness_total_only_alias_is_not_benchmark_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-total-only-harness",
                "import json\nprint(json.dumps({'total_count': 3}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertIsNone(event["score"])
            self.assertEqual(event["error_code"], "harness_unparsed")
            self.assertIsNone(event["tests"]["total"])
            self.assertIsNone(event["tests"]["passed"])
            self.assertIsNone(event["tests"]["failed"])

    def test_external_harness_zero_exit_score_only_perfect_result_can_pass(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-green-score-harness",
                "import json\nprint(json.dumps({'score': 1.0}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"command": [str(tool)]}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "aider-polyglot",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "passed")
            self.assertEqual(event["score"], 1.0)
            self.assertIsNone(event["error_code"])

    def test_harness_command_templates_model_and_base_url(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-model-harness",
                "import json, sys\nprint('ARGS=' + ' '.join(sys.argv[1:]))\nprint(json.dumps({'total': 1, 'passed': 1, 'failed': 0, 'score': 1.0}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(
                json.dumps(
                    {
                        "harness": {
                            "command": [str(tool), "--model", "{model}", "--base-url", "{base_url}", "--dataset", "{dataset}"],
                            "model": "local-qwen",
                            "base_url": "http://127.0.0.1:8080/v1",
                        }
                    }
                ),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--dataset",
                "humaneval",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "passed")
            raw_artifact = Path(event["artifacts"][0]["path"])
            raw_output = raw_artifact.read_text(encoding="utf-8")
            self.assertIn("--model local-qwen", raw_output)
            self.assertIn("--base-url http://127.0.0.1:8080/v1", raw_output)
            self.assertIn("--dataset humaneval", raw_output)

    def test_external_harness_does_not_inherit_parent_secret_env_by_default(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-env-harness",
                "import json, os\nprint('HAS_SECRET=' + ('yes' if os.environ.get('LEAKED_WORKER_SECRET') else 'no'))\nprint('VISIBLE=' + os.environ.get('VISIBLE_FLAG', 'missing'))\nprint(json.dumps({'total': 1, 'passed': 1, 'failed': 0, 'score': 1.0}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(
                json.dumps({"harness": {"command": [str(tool)], "env": {"VISIBLE_FLAG": "ok"}}}),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
                env_overrides={"LEAKED_WORKER_SECRET": "secret-value"},
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            raw_output = Path(self.final_event(output)["artifacts"][0]["path"]).read_text(encoding="utf-8")
            self.assertIn("HAS_SECRET=no", raw_output)
            self.assertIn("VISIBLE=ok", raw_output)
            self.assertNotIn("secret-value", raw_output)

    def test_external_harness_env_passthrough_is_explicit(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-env-passthrough-harness",
                "import json, os\nprint('HAS_SECRET=' + ('yes' if os.environ.get('LEAKED_WORKER_SECRET') else 'no'))\nprint(json.dumps({'total': 1, 'passed': 1, 'failed': 0, 'score': 1.0}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(
                json.dumps({"harness": {"command": [str(tool)], "env_passthrough": ["LEAKED_WORKER_SECRET"]}}),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
                env_overrides={"LEAKED_WORKER_SECRET": "secret-value"},
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            raw_output = Path(self.final_event(output)["artifacts"][0]["path"]).read_text(encoding="utf-8")
            self.assertIn("HAS_SECRET=yes", raw_output)
            self.assertNotIn("secret-value", raw_output)

    def test_external_harness_output_redacts_passthrough_secret_values(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            leaked_secret = "sk-testsecretvalue000000000000000000"
            tool = self.write_tool(
                temp_dir,
                "fake-secret-printing-harness",
                "import json, os\nprint('SECRET=' + os.environ.get('LEAKED_WORKER_SECRET', 'missing'))\nprint(json.dumps({'total': 1, 'passed': 1, 'failed': 0, 'score': 1.0}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(
                json.dumps({"harness": {"command": [str(tool)], "env_passthrough": ["LEAKED_WORKER_SECRET"]}}),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
                env_overrides={"LEAKED_WORKER_SECRET": leaked_secret},
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            output_text = output.read_text(encoding="utf-8")
            raw_output = Path(self.final_event(output)["artifacts"][0]["path"]).read_text(encoding="utf-8")
            self.assertNotIn(leaked_secret, output_text)
            self.assertNotIn(leaked_secret, raw_output)
            self.assertIn("<redacted", raw_output)

    def test_literal_secret_harness_env_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            tool = self.write_tool(
                temp_dir,
                "fake-secret-env-harness",
                "import json\nprint(json.dumps({'total': 1, 'passed': 1, 'failed': 0, 'score': 1.0}))\n",
            )
            target_config = temp_dir / "target.json"
            target_config.write_text(
                json.dumps({"harness": {"command": [str(tool)], "env": {"OPENAI_API_KEY": "sk-secret"}}}),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertEqual(event["error_code"], "configuration_secret_env")
            self.assertIn("env_passthrough", event["error_message"])

    def test_missing_harness_command_is_actionable(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "swebench",
                "--subset",
                "lite-small",
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertEqual(event["error_code"], "configuration_missing")
            self.assertIn("harness.command", event["error_message"])

    def test_worker_imports_existing_jsonl_result_without_running_harness(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "existing-results.jsonl"
            import_text = "\n".join(
                [
                    json.dumps({"case": "one", "passed": True}),
                    json.dumps({"tests": {"total": 4, "passed": 3, "failed": 1}, "score": 0.75}),
                ]
            ) + "\n"
            imported.write_text(import_text, encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="jsonl", summary_source="json")
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["tests"]["total"], 4)
            self.assertEqual(event["tests"]["passed"], 3)
            self.assertEqual(event["tests"]["failed"], 1)
            expected_hash = hashlib.sha256(import_text.encode("utf-8")).hexdigest()
            self.assertEqual(event["import_file_details"][0]["path"], "existing-results.jsonl")
            self.assertEqual(event["import_file_details"][0]["sha256"], expected_hash)
            self.assertEqual(event["import_file_details"][0]["read_sha256"], expected_hash)
            raw_artifact = Path(event["artifacts"][0]["path"])
            self.assertIn("existing-results.jsonl", raw_artifact.read_text(encoding="utf-8"))

    def test_worker_import_redacts_secret_values_from_artifacts_and_jsonl(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            leaked_token = "hf_testsecretvalue000000000000000000"
            imported = temp_dir / "existing-results.jsonl"
            imported.write_text(
                "\n".join(
                    [
                        json.dumps({"note": f"token={leaked_token}"}),
                        json.dumps({"tests": {"total": 1, "passed": 1, "failed": 0}, "score": 1.0}),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="jsonl", summary_source="json")
            self.assertEqual(event["status"], "passed")
            output_text = output.read_text(encoding="utf-8")
            raw_output = Path(event["artifacts"][0]["path"]).read_text(encoding="utf-8")
            self.assertNotIn(leaked_token, output_text)
            self.assertNotIn(leaked_token, raw_output)
            self.assertIn("<redacted", output_text)
            self.assertIn("<redacted", raw_output)

    def test_worker_imports_existing_result_from_target_config_directory(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imports = temp_dir / "imports"
            imports.mkdir()
            (imports / "summary.log").write_text("pass@1: 1.0\n2 passed, 0 failed\n", encoding="utf-8")
            target_config = temp_dir / "target.json"
            target_config.write_text(json.dumps({"harness": {"import_path": "imports"}}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--target-config",
                str(target_config),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(
                event,
                import_format="text",
                import_source="directory",
                import_files=1,
                summary_source="text",
            )
            self.assertEqual(event["status"], "passed")
            self.assertEqual(event["score"], 1.0)
            self.assertEqual(event["tests"]["total"], 2)
            self.assertEqual(event["tests"]["passed"], 2)
            self.assertEqual(event["tests"]["failed"], 0)

    def test_worker_directory_import_prioritizes_structured_summary_before_large_logs(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imports = temp_dir / "imports"
            imports.mkdir()
            (imports / "a-large.log").write_text("x" * 3_001_024, encoding="utf-8")
            (imports / "summary.json").write_text(
                json.dumps({"tests": {"total": 2, "passed": 2, "failed": 0}, "score": 1.0}) + "\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--import-path",
                str(imports),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(
                event,
                import_format="mixed",
                import_source="directory",
                import_files=2,
                summary_source="json",
            )
            self.assertEqual(event["status"], "passed")
            self.assertEqual(event["score"], 1.0)
            self.assertEqual(event["tests"]["total"], 2)
            self.assertEqual(event["import_read_files"][0], "summary.json")
            self.assertIn("a-large.log", event["import_read_files"])
            self.assertTrue(event["import_truncated"])
            self.assertGreater(event["metrics"]["import_truncated_bytes"], 0)
            raw_output = Path(event["artifacts"][0]["path"]).read_text(encoding="utf-8")
            self.assertLess(raw_output.index("summary.json"), raw_output.index("a-large.log"))

    def test_worker_imports_csv_summary_result(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "suite-summary.csv"
            imported.write_text(
                "total,passed,failed,accuracy\n4,3,1,0.75\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="csv", summary_source="csv")
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["tests"]["total"], 4)
            self.assertEqual(event["tests"]["passed"], 3)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_worker_imports_csv_percent_score_as_normalized_result(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "suite-percent-summary.csv"
            imported.write_text(
                "total,passed,failed,accuracy\n4,3,1,75\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="csv", summary_source="csv")
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["tests"]["total"], 4)

    def test_worker_imports_csv_common_summary_aliases(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "suite-alias-summary.csv"
            imported.write_text(
                "total examples,num correct,num incorrect,exact match\n4,3,1,75%\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="csv", summary_source="csv")
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["tests"]["total"], 4)
            self.assertEqual(event["tests"]["passed"], 3)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_worker_imports_csv_case_results_from_directory(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imports = temp_dir / "imports"
            imports.mkdir()
            (imports / "cases.csv").write_text(
                "instance_id,resolved\nrepo__one,true\nrepo__two,false\nrepo__three,true\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "swebench",
                "--import-path",
                str(imports),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(
                event,
                import_format="csv",
                import_source="directory",
                import_files=1,
                summary_source="csv",
            )
            self.assertEqual(event["status"], "failed")
            self.assertAlmostEqual(event["score"], 2 / 3)
            self.assertEqual(event["tests"]["total"], 3)
            self.assertEqual(event["tests"]["passed"], 2)
            self.assertEqual(event["tests"]["failed"], 1)
            raw_artifact = Path(event["artifacts"][0]["path"])
            self.assertIn("cases.csv", raw_artifact.read_text(encoding="utf-8"))

    def test_worker_imports_nested_json_test_cases_result(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "nested-tests.json"
            imported.write_text(
                json.dumps(
                    {
                        "suite": "sample",
                        "tests": {
                            "cases": [
                                {"id": "one", "result": "passed"},
                                {"id": "two", "result": "failed"},
                                {"id": "three", "success": True},
                            ]
                        },
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="json", summary_source="json")
            self.assertEqual(event["status"], "failed")
            self.assertAlmostEqual(event["score"], 2 / 3)
            self.assertEqual(event["tests"]["total"], 3)
            self.assertEqual(event["tests"]["passed"], 2)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_worker_imports_junit_xml_result(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "junit.xml"
            imported.write_text(
                """<?xml version="1.0" encoding="utf-8"?>
<testsuites>
  <testsuite name="suite-a" tests="3" failures="1" errors="0" skipped="0">
    <testcase classname="bench" name="one"/>
    <testcase classname="bench" name="two"><failure message="bad"/></testcase>
    <testcase classname="bench" name="three"/>
  </testsuite>
  <testsuite name="suite-b" tests="1" failures="0" errors="0" skipped="0">
    <testcase classname="bench" name="four"/>
  </testsuite>
</testsuites>
""",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="xml", summary_source="junit_xml")
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.75)
            self.assertEqual(event["tests"]["total"], 4)
            self.assertEqual(event["tests"]["passed"], 3)
            self.assertEqual(event["tests"]["failed"], 1)
            raw_artifact = Path(event["artifacts"][0]["path"])
            self.assertIn("junit.xml", raw_artifact.read_text(encoding="utf-8"))

    def test_worker_directory_import_records_unsupported_files_without_using_them(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imports = temp_dir / "imports"
            imports.mkdir()
            (imports / "summary.json").write_text(
                json.dumps({"total": 2, "passed": 2, "failed": 0}),
                encoding="utf-8",
            )
            (imports / "notes.md").write_text("# human notes\n", encoding="utf-8")
            nested = imports / "screenshots"
            nested.mkdir()
            (nested / "chart.png").write_bytes(b"not really an image")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--import-path",
                str(imports),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(
                event,
                import_format="json",
                import_source="directory",
                import_files=1,
                summary_source="json",
            )
            self.assertEqual(event["status"], "passed")
            self.assertEqual(event["import_read_files"], ["summary.json"])
            self.assertEqual(event["import_total_files"], 1)
            self.assertEqual(event["import_omitted_files"], 0)
            self.assertEqual(event["import_unsupported_file_count"], 2)
            self.assertEqual(event["metrics"]["import_unsupported_file_count"], 2)
            self.assertEqual(set(event["import_unsupported_files"]), {"notes.md", "screenshots/chart.png"})
            raw_output = Path(event["artifacts"][0]["path"]).read_text(encoding="utf-8")
            self.assertIn("[ignored 2 unsupported file(s):", raw_output)
            self.assertIn("notes.md", raw_output)
            warnings = [item["message"] for item in event["safety"]["diagnostics"] if item["level"] == "warn"]
            self.assertTrue(any("ignored 2 unsupported file(s)" in warning for warning in warnings))

    def test_worker_imports_junit_xml_without_suite_counts(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "junit-cases.xml"
            imported.write_text(
                """<testsuite name="case-only">
  <testcase classname="bench" name="one"/>
  <testcase classname="bench" name="two"><error message="boom"/></testcase>
  <testcase classname="bench" name="three"><skipped/></testcase>
</testsuite>
""",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="xml", summary_source="junit_xml")
            self.assertEqual(event["status"], "failed")
            self.assertAlmostEqual(event["score"], 1 / 3)
            self.assertEqual(event["tests"]["total"], 3)
            self.assertEqual(event["tests"]["passed"], 1)
            self.assertEqual(event["tests"]["failed"], 2)

    def test_worker_imports_jsonl_item_results_without_summary_record(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "swebench-results.jsonl"
            imported.write_text(
                "\n".join(
                    [
                        json.dumps({"instance_id": "repo__one", "resolved": True}),
                        json.dumps({"instance_id": "repo__two", "resolved": False}),
                        json.dumps({"instance_id": "repo__three", "status": "passed"}),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "swebench",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="jsonl", summary_source="json_items")
            self.assertEqual(event["status"], "failed")
            self.assertAlmostEqual(event["score"], 2 / 3)
            self.assertEqual(event["tests"]["total"], 3)
            self.assertEqual(event["tests"]["passed"], 2)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_worker_rejects_truncated_jsonl_item_import_as_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "large-results.jsonl"
            imported.write_text(
                ("x" * 3_001_024)
                + "\n"
                + "\n".join(
                    [
                        json.dumps({"instance_id": "repo__tail_one", "resolved": True}),
                        json.dumps({"instance_id": "repo__tail_two", "resolved": True}),
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "swebench",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="jsonl", summary_source="json_items")
            self.assertTrue(event["import_truncated"])
            self.assertGreater(event["metrics"]["import_truncated_bytes"], 0)
            self.assertEqual(event["status"], "error")
            self.assertIsNone(event["score"])
            self.assertEqual(event["error_code"], "result_import_truncated")
            self.assertIn("complete aggregate summary", event["error_message"])
            raw_output = Path(event["artifacts"][0]["path"]).read_text(encoding="utf-8")
            self.assertIn("[truncated first", raw_output)

    def test_worker_imports_nested_suite_result_map(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "suite-report.json"
            imported.write_text(
                json.dumps(
                    {
                        "metrics": {"accuracy": 0.667},
                        "results": {
                            "case-1": {"status": "passed"},
                            "case-2": {"status": "failed"},
                            "case-3": {"resolved": True},
                        },
                    }
                ),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "aider-polyglot",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="json", summary_source="json")
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.667)
            self.assertEqual(event["tests"]["total"], 3)
            self.assertEqual(event["tests"]["passed"], 2)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_worker_imports_resolved_unresolved_id_sets(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "resolved-summary.json"
            imported.write_text(
                json.dumps({"resolved": ["one", "two"], "unresolved": ["three"]}),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "swebench",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="json", summary_source="json")
            self.assertEqual(event["status"], "failed")
            self.assertAlmostEqual(event["score"], 2 / 3)
            self.assertEqual(event["tests"]["total"], 3)
            self.assertEqual(event["tests"]["passed"], 2)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_worker_imports_json_common_summary_aliases(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "alias-summary.json"
            imported.write_text(
                json.dumps(
                    {
                        "total-count": 5,
                        "num correct": 4,
                        "num-incorrect": 1,
                        "success rate": "80%",
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(event, import_format="json", summary_source="json")
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["score"], 0.8)
            self.assertEqual(event["tests"]["total"], 5)
            self.assertEqual(event["tests"]["passed"], 4)
            self.assertEqual(event["tests"]["failed"], 1)

    def test_worker_rejects_import_path_outside_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir, tempfile.TemporaryDirectory() as outside_raw:
            temp_dir = Path(raw_dir)
            outside = Path(outside_raw) / "summary.json"
            outside.write_text(json.dumps({"total": 1, "passed": 1, "failed": 0}), encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(outside),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertEqual(event["error_code"], "configuration_invalid")
            self.assertIn("inside the worker workspace", event["error_message"])

    def test_worker_rejects_unsupported_direct_import_file_type(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imported = temp_dir / "results.bin"
            imported.write_bytes(b"\x00\x01not a supported benchmark result")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertEqual(event["error_code"], "import_invalid")
            self.assertIn("unsupported result file type", event["error_message"])
            self.assertIn(".jsonl", event["error_message"])

    def test_worker_rejects_symlinked_direct_import_path(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            real_result = temp_dir / "real-summary.json"
            real_result.write_text(json.dumps({"total": 1, "passed": 1, "failed": 0}), encoding="utf-8")
            imported = temp_dir / "summary-link.json"
            self.symlink_or_skip(imported, real_result)
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imported),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertEqual(event["error_code"], "import_invalid")
            self.assertIn("must not be a symlink", event["error_message"])

    def test_worker_rejects_symlinked_file_in_import_directory(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir, tempfile.TemporaryDirectory() as outside_raw:
            temp_dir = Path(raw_dir)
            imports = temp_dir / "imports"
            imports.mkdir()
            outside = Path(outside_raw) / "summary.json"
            outside.write_text(json.dumps({"total": 1, "passed": 1, "failed": 0}), encoding="utf-8")
            try:
                (imports / "summary.json").symlink_to(outside)
            except OSError as exc:
                self.skipTest(f"symlinks are not available in this environment: {exc}")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "evalplus",
                "--import-path",
                str(imports),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2)
            event = self.final_event(output)
            self.assertEqual(event["status"], "error")
            self.assertEqual(event["error_code"], "import_invalid")
            self.assertIn("symlinked result file", event["error_message"])

    def test_worker_directory_import_rejects_truncated_text_summary_as_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            imports = temp_dir / "imports"
            imports.mkdir()
            summary = "\npass@1: 1.0\n1 passed, 0 failed\n"
            (imports / "large-a.log").write_text(("x" * 3_001_024) + summary, encoding="utf-8")
            (imports / "large-b.log").write_text("pass@1: 0.0\n0 passed, 1 failed\n", encoding="utf-8")
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "terminal-bench",
                "--import-path",
                str(imports),
                "--workspace",
                str(temp_dir),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 2, completed.stderr)
            event = self.final_event(output)
            self.assert_import_metadata(
                event,
                import_format="text",
                import_source="directory",
                import_files=1,
                summary_source="text",
            )
            self.assertTrue(event["import_truncated"])
            self.assertEqual(event["import_total_files"], 2)
            self.assertEqual(event["import_omitted_files"], 1)
            self.assertEqual(event["metrics"]["import_truncated"], 1)
            self.assertEqual(event["metrics"]["import_total_file_count"], 2)
            self.assertEqual(event["metrics"]["import_omitted_file_count"], 1)
            self.assertGreater(event["metrics"]["import_truncated_bytes"], 0)
            self.assertEqual(event["status"], "error")
            self.assertIsNone(event["score"])
            self.assertEqual(event["error_code"], "result_import_truncated")
            self.assertIn("complete JSON, CSV, or JUnit summary", event["error_message"])
            raw_artifact = Path(event["artifacts"][0]["path"])
            raw_output = raw_artifact.read_text(encoding="utf-8")
            self.assertIn("[truncated first", raw_output)
            self.assertIn("[omitted 1 supported result file(s) after import limits]", raw_output)
            warnings = [item["message"] for item in event["safety"]["diagnostics"] if item["level"] == "warn"]
            self.assertTrue(any("was truncated" in warning for warning in warnings))
            errors = [item["message"] for item in event["safety"]["diagnostics"] if item["level"] == "error"]
            self.assertTrue(any("complete aggregate summary" in error for error in errors))

    def test_security_runner_parses_bandit_json_findings(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            vulnerable = workspace / "vulnerable.py"
            vulnerable.write_text("value = eval('2 + 2')\n", encoding="utf-8")
            payload = {
                "results": [
                    {
                        "test_id": "B307",
                        "test_name": "blacklist",
                        "issue_text": "Use of possibly insecure function",
                        "issue_severity": "HIGH",
                        "issue_confidence": "HIGH",
                        "filename": str(vulnerable),
                        "line_number": 1,
                        "issue_cwe": {"id": 78},
                    }
                ]
            }
            self.write_tool(
                temp_dir,
                "bandit",
                f"import json, sys\nprint(json.dumps({payload!r}))\nsys.exit(1)\n",
            )
            output = temp_dir / "worker.jsonl"
            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "bandit",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
                env_overrides={"PATH": str(temp_dir) + os.pathsep + os.environ.get("PATH", "")},
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["metrics"]["finding_count"], 1)
            self.assertEqual(event["metrics"]["files_scanned"], 1)
            finding = event["safety"]["static_analysis_findings"][0]
            self.assertEqual(finding["source"], "bandit")
            self.assertEqual(finding["rule_id"], "B307")
            self.assertEqual(finding["severity"], "high")
            self.assertEqual(finding["confidence"], "high")
            self.assertEqual(finding["cwe"], 78)
            self.assertEqual(finding["path"], "vulnerable.py")

    def test_security_runner_parses_pip_audit_json_findings(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            requirements = workspace / "requirements.txt"
            requirements.write_text("flask==0.12\n", encoding="utf-8")
            payload = {
                "dependencies": [
                    {
                        "name": "flask",
                        "version": "0.12",
                        "vulns": [
                            {
                                "id": "PYSEC-2019-179",
                                "description": "Example Flask vulnerability",
                                "fix_versions": ["2.2.5"],
                                "aliases": ["CVE-2019-1010083"],
                            }
                        ],
                    }
                ]
            }
            self.write_tool(
                temp_dir,
                "pip-audit",
                f"import json, sys\nprint(json.dumps({payload!r}))\nsys.exit(1)\n",
            )
            output = temp_dir / "worker.jsonl"
            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "dependency",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
                env_overrides={"PATH": str(temp_dir) + os.pathsep + os.environ.get("PATH", "")},
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["metrics"]["finding_count"], 1)
            self.assertEqual(event["metrics"]["files_scanned"], 1)
            finding = event["safety"]["static_analysis_findings"][0]
            self.assertEqual(finding["source"], "pip-audit")
            self.assertEqual(finding["rule_id"], "PYSEC-2019-179")
            self.assertEqual(finding["severity"], "high")
            self.assertEqual(finding["package"], "flask")
            self.assertEqual(finding["installed_version"], "0.12")
            self.assertEqual(finding["path"], "requirements.txt")
            self.assertEqual(finding["line"], 1)

    def test_security_runner_dependency_fallback_checks_python_and_node_manifests(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            (workspace / "requirements.txt").write_text("PyYAML==3.13\n", encoding="utf-8")
            (workspace / "package.json").write_text(
                json.dumps({"dependencies": {"lodash": "4.17.20"}}),
                encoding="utf-8",
            )
            output = temp_dir / "worker.jsonl"
            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "dependency",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
                env_overrides={"PATH": str(temp_dir)},
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["metrics"]["finding_count"], 2)
            self.assertEqual(event["metrics"]["files_scanned"], 2)
            sources = {finding["source"] for finding in event["safety"]["static_analysis_findings"]}
            packages = {finding["package"] for finding in event["safety"]["static_analysis_findings"]}
            self.assertEqual(sources, {"dependency-fallback"})
            self.assertEqual(packages, {"pyyaml", "lodash"})

    def test_security_runner_dependency_fallback_ignores_symlinked_manifests(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir, tempfile.TemporaryDirectory() as outside_raw:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            outside = Path(outside_raw) / "requirements.txt"
            outside.write_text("PyYAML==3.13\n", encoding="utf-8")
            self.symlink_or_skip(workspace / "requirements.txt", outside)
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "dependency",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
                env_overrides={"PATH": str(temp_dir)},
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "passed")
            self.assertEqual(event["metrics"]["finding_count"], 0)
            self.assertEqual(event["metrics"]["files_scanned"], 0)

    def test_security_runner_fallback_ignores_symlinked_files_outside_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir, tempfile.TemporaryDirectory() as outside_raw:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            (workspace / "safe.py").write_text("print('ok')\n", encoding="utf-8")
            outside = Path(outside_raw) / "unsafe.py"
            outside.write_text("eval('2 + 2')\n", encoding="utf-8")
            self.symlink_or_skip(workspace / "unsafe.py", outside)
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "fallback",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "passed")
            self.assertEqual(event["metrics"]["finding_count"], 0)
            self.assertEqual(event["metrics"]["files_scanned"], 1)

    def test_security_runner_secret_fallback_redacts_matches(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            leaked_secret = "sk-testsecretvalue000000000000000000"
            (workspace / "config.py").write_text(f"OPENAI_API_KEY = '{leaked_secret}'\n", encoding="utf-8")
            output = temp_dir / "worker.jsonl"
            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "secrets",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
                env_overrides={"PATH": str(temp_dir)},
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            output_text = output.read_text(encoding="utf-8")
            self.assertNotIn(leaked_secret, output_text)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["metrics"]["finding_count"], 1)
            finding = event["safety"]["static_analysis_findings"][0]
            self.assertEqual(finding["source"], "secret-fallback")
            self.assertEqual(finding["path"], "config.py")
            self.assertEqual(finding["line"], 1)
            self.assertTrue(finding["redacted"])
            self.assertIn("fingerprint", finding)

    def test_security_runner_secret_fallback_ignores_symlinked_files_outside_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir, tempfile.TemporaryDirectory() as outside_raw:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            outside = Path(outside_raw) / "config.py"
            outside.write_text("OPENAI_API_KEY = 'sk-testsecretvalue000000000000000000'\n", encoding="utf-8")
            self.symlink_or_skip(workspace / "config.py", outside)
            output = temp_dir / "worker.jsonl"

            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "secrets",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
                env_overrides={"PATH": str(temp_dir)},
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            event = self.final_event(output)
            self.assertEqual(event["status"], "passed")
            self.assertEqual(event["metrics"]["finding_count"], 0)
            self.assertEqual(event["metrics"]["files_scanned"], 0)

    def test_security_runner_parses_gitleaks_json_without_secret_value(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            temp_dir = Path(raw_dir)
            workspace = temp_dir / "workspace"
            workspace.mkdir()
            leaked_secret = "hf_testsecretvalue000000000000000000"
            (workspace / "settings.env").write_text(f"HF_TOKEN={leaked_secret}\n", encoding="utf-8")
            self.write_tool(
                temp_dir,
                "gitleaks",
                """
import json
import pathlib
import sys
args = sys.argv[1:]
report_path = pathlib.Path(args[args.index('--report-path') + 1])
report_path.write_text(json.dumps([{
    'RuleID': 'huggingface-token',
    'Description': 'Hugging Face token',
    'File': 'settings.env',
    'StartLine': 1,
    'Secret': 'REDACTED'
}]), encoding='utf-8')
sys.exit(1)
""",
            )
            output = temp_dir / "worker.jsonl"
            completed = self.run_worker(
                temp_dir,
                "--kind",
                "security",
                "--tool",
                "secrets",
                "--workspace",
                str(workspace),
                "--output",
                str(output),
                env_overrides={"PATH": str(temp_dir) + os.pathsep + os.environ.get("PATH", "")},
            )

            self.assertEqual(completed.returncode, 1, completed.stderr)
            output_text = output.read_text(encoding="utf-8")
            self.assertNotIn(leaked_secret, output_text)
            self.assertNotIn("REDACTED", output_text)
            event = self.final_event(output)
            self.assertEqual(event["status"], "failed")
            self.assertEqual(event["metrics"]["finding_count"], 1)
            finding = event["safety"]["static_analysis_findings"][0]
            self.assertEqual(finding["source"], "gitleaks")
            self.assertEqual(finding["rule_id"], "huggingface-token")
            self.assertEqual(finding["path"], "settings.env")
            self.assertTrue(finding["redacted"])


if __name__ == "__main__":
    unittest.main()
