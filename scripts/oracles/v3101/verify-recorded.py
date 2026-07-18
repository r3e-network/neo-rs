#!/usr/bin/env python3
"""Verify enriched fixtures against freshly recorded C# oracle output."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def load(path: Path) -> dict:
    with path.open(encoding="utf-8") as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise SystemExit(f"{path}: expected a JSON object")
    return value


def indexed_cases(document: dict, path: Path) -> dict[str, dict]:
    cases = document.get("cases")
    if not isinstance(cases, list):
        raise SystemExit(f"{path}: cases must be an array")
    indexed: dict[str, dict] = {}
    for case in cases:
        if not isinstance(case, dict) or not isinstance(case.get("id"), str):
            raise SystemExit(f"{path}: every case must have a string id")
        case_id = case["id"]
        if case_id in indexed:
            raise SystemExit(f"{path}: duplicate case id {case_id}")
        indexed[case_id] = case
    return indexed


def main() -> int:
    if len(sys.argv) != 3:
        raise SystemExit("usage: verify-recorded.py FIXTURE RECORDED_OUTPUT")

    fixture_path = Path(sys.argv[1])
    recorded_path = Path(sys.argv[2])
    fixture = load(fixture_path)
    recorded = load(recorded_path)

    for key in ("repository", "commit", "version"):
        expected = fixture.get("oracle", {}).get(key)
        actual = recorded.get("oracle", {}).get(key)
        if expected != actual:
            raise SystemExit(
                f"oracle {key} mismatch: fixture={expected!r}, recorded={actual!r}"
            )

    expected_cases = indexed_cases(fixture, fixture_path)
    actual_cases = indexed_cases(recorded, recorded_path)
    if expected_cases.keys() != actual_cases.keys():
        missing = sorted(expected_cases.keys() - actual_cases.keys())
        extra = sorted(actual_cases.keys() - expected_cases.keys())
        raise SystemExit(f"case set mismatch: missing={missing}, extra={extra}")

    mismatches = []
    for case_id, expected in expected_cases.items():
        actual = actual_cases[case_id]
        if expected.get("observed") != actual.get("observed"):
            mismatches.append(case_id)
    if mismatches:
        raise SystemExit(f"observed result mismatch: {', '.join(mismatches)}")

    print(f"verified {len(expected_cases)} recorded cases from {recorded_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
