#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

import yaml
from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def load_yaml(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"{path} must contain a YAML object")
    return data


def validate_instance(schema: dict[str, Any], path: Path) -> list[str]:
    validator = Draft202012Validator(schema)
    errors: list[str] = []
    for error in sorted(validator.iter_errors(load_yaml(path)), key=lambda item: list(item.path)):
        location = "$" + "".join(f".{part}" for part in error.path)
        errors.append(f"{path.relative_to(ROOT)}: {location}: {error.message}")
    return errors


def semantic_benchmark_pack_errors(pack_path: Path) -> list[str]:
    errors: list[str] = []
    pack = load_yaml(pack_path)
    task_values = pack.get("tasks")
    if not isinstance(task_values, list):
        return errors
    task_paths = [pack_path.parent / str(value) for value in task_values]
    tasks: list[dict[str, Any]] = []
    for task_path in task_paths:
        try:
            tasks.append(load_yaml(task_path))
        except Exception as exc:
            errors.append(f"{pack_path.relative_to(ROOT)}: task {task_path.relative_to(ROOT)} could not be loaded for semantic validation: {exc}")
    if not tasks:
        return errors

    tags = {str(tag).lower() for tag in pack.get("tags", []) if isinstance(tag, str)}
    all_prompt = all(task.get("type") == "prompt" for task in tasks)
    prompt_comparison_pack = (
        all_prompt
        and len(tasks) >= 3
        and "llm" in tags
        and "prompt" in tags
        and "smoke" not in tags
        and "connectivity" not in tags
    )
    if not prompt_comparison_pack:
        return errors

    calibration = pack.get("calibration")
    location = f"{pack_path.relative_to(ROOT)}: $.calibration"
    if not isinstance(calibration, dict):
        errors.append(f"{location}: prompt comparison packs must include calibration metadata")
        return errors

    status = str(calibration.get("status", "")).strip()
    if status not in {"pilot", "reviewed", "calibrated"}:
        errors.append(f"{location}.status: prompt comparison packs must be pilot, reviewed, or calibrated")
    last_reviewed = str(calibration.get("last_reviewed", "")).strip()
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", last_reviewed):
        errors.append(f"{location}.last_reviewed: prompt comparison packs must record a YYYY-MM-DD review date")
    review_scope = str(calibration.get("review_scope", "")).strip()
    if review_scope not in {"contract_review", "pilot_runs", "baseline_runs", "production_validation"}:
        errors.append(f"{location}.review_scope: prompt comparison packs must state contract_review, pilot_runs, baseline_runs, or production_validation")
    notes = str(calibration.get("notes", "")).strip()
    if len(notes) < 40:
        errors.append(f"{location}.notes: prompt comparison packs must include concise review notes")

    gates = calibration.get("quality_gates")
    required_gates = {
        "local_cloud_baseline_pair",
        "provider_confirmed_model_identity",
        "complete_pack_task_coverage",
        "min_3_repetitions_per_task_target",
        "cost_metrics_for_cloud_targets",
        "single_generation_policy",
    }
    gate_values = {str(gate) for gate in gates} if isinstance(gates, list) else set()
    missing_gates = sorted(required_gates - gate_values)
    if missing_gates:
        errors.append(f"{location}.quality_gates: prompt comparison packs are missing required gate(s): {', '.join(missing_gates)}")

    sample_size = calibration.get("sample_size")
    baseline_models = calibration.get("baseline_models")
    if status == "calibrated":
        if not isinstance(sample_size, int) or sample_size <= 0:
            errors.append(f"{location}.sample_size: calibrated packs must record a positive sample size")
        if not isinstance(baseline_models, list) or len({str(model).strip() for model in baseline_models if str(model).strip()}) < 2:
            errors.append(f"{location}.baseline_models: calibrated packs must list at least two baseline models")

    return errors


def main() -> int:
    schema_paths = sorted((ROOT / "specs" / "schemas").glob("*.schema.json"))
    schemas = {path.name: load_json(path) for path in schema_paths}

    failures: list[str] = []
    for path, schema in ((ROOT / "specs" / "schemas" / name, schema) for name, schema in schemas.items()):
        try:
            Draft202012Validator.check_schema(schema)
        except Exception as exc:  # jsonschema exposes several validation exception types.
            failures.append(f"{path.relative_to(ROOT)}: invalid JSON Schema: {exc}")

    checks = [
        ("adapter.schema.json", sorted((ROOT / "adapters").glob("**/*.yaml"))),
        ("benchmark_pack.schema.json", sorted((ROOT / "benchmark-packs").glob("*/pack.yaml"))),
        ("task.schema.json", sorted((ROOT / "benchmark-packs").glob("*/tasks/*.yaml"))),
    ]
    for schema_name, paths in checks:
        for path in paths:
            failures.extend(validate_instance(schemas[schema_name], path))

    for path in sorted((ROOT / "benchmark-packs").glob("*/pack.yaml")):
        failures.extend(semantic_benchmark_pack_errors(path))

    if failures:
        print("Schema validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    instance_count = sum(len(paths) for _, paths in checks)
    print(f"Validated {len(schema_paths)} schemas and {instance_count} YAML files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
