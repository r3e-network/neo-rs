#!/usr/bin/env python3
"""Compare NeoVM differential runner outputs.

The Rust and C# runners emit the same result envelope.  This tool compares the
results without building either runner, making it useful in CI and when one
runner's output has already been captured.

Usage examples::

    python scripts/vm-diff.py rust-results.json csharp-results.json
    python scripts/vm-diff.py rust-results.json csharp-results.json --report report.json
    python scripts/vm-diff.py --json rust-results.json csharp-results.json

Exit status is 0 when every vector matches, 1 when vectors differ, and 2 when
an input or command-line error prevents comparison.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Sequence


COMPARE_FIELDS: tuple[str, ...] = ("name", "state", "stack", "fault", "harness_error")


class InputError(ValueError):
    """Raised when a runner output does not follow the expected JSON schema."""


def _load_json(path: Path) -> dict[str, Any]:
    """Read and validate one runner output file."""
    try:
        raw = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as exc:
        raise InputError(f"cannot read {path}: {exc}") from exc
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise InputError(f"invalid JSON in {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise InputError(f"{path}: top-level JSON value must be an object")
    results = value.get("results")
    if not isinstance(results, list):
        raise InputError(f"{path}: missing 'results' array")

    seen: set[str] = set()
    for index, result in enumerate(results):
        if not isinstance(result, dict):
            raise InputError(f"{path}: results[{index}] must be an object")
        missing = [field for field in ("name", "state", "stack", "fault") if field not in result]
        if missing:
            names = ", ".join(repr(field) for field in missing)
            raise InputError(f"{path}: results[{index}] missing {names}")
        name = result["name"]
        if not isinstance(name, str) or not name:
            raise InputError(f"{path}: results[{index}].name must be a non-empty string")
        if name in seen:
            raise InputError(f"{path}: duplicate result name {name!r}")
        seen.add(name)
        if not isinstance(result["state"], str):
            raise InputError(f"{path}: results[{index}].state must be a string")
        if not isinstance(result["stack"], list):
            raise InputError(f"{path}: results[{index}].stack must be an array")
        if result["fault"] is not None and not isinstance(result["fault"], str):
            raise InputError(f"{path}: results[{index}].fault must be a string or null")
        if "harness_error" in result and result["harness_error"] is not None and not isinstance(
            result["harness_error"], str
        ):
            raise InputError(
                f"{path}: results[{index}].harness_error must be a string or null"
            )
    return value


def load_results(path: str | Path) -> dict[str, Any]:
    """Public wrapper for loading a validated runner output."""
    return _load_json(Path(path))


def _json_equal(left: Any, right: Any) -> bool:
    """Compare JSON values strictly, including scalar JSON types.

    Python considers ``True == 1``; JSON does not.  Recursive type checks avoid
    accidentally accepting such a difference in a stack item or nested value.
    Object key order is intentionally ignored, while array order is significant.
    """
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(
            _json_equal(left[key], right[key]) for key in left
        )
    if isinstance(left, list):
        return len(left) == len(right) and all(
            _json_equal(item_left, item_right) for item_left, item_right in zip(left, right)
        )
    return left == right


def _result_map(document: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Index validated results by vector name."""
    indexed: dict[str, dict[str, Any]] = {}
    for result in document["results"]:
        # Missing harness_error is how the Rust serde output represents null.
        normalized = dict(result)
        normalized.setdefault("harness_error", None)
        indexed[normalized["name"]] = normalized
    return indexed


def compare_results(left: dict[str, Any], right: dict[str, Any]) -> dict[str, Any]:
    """Compare two validated documents and return a serializable report."""
    left_map = _result_map(left)
    right_map = _result_map(right)
    names = sorted(set(left_map) | set(right_map))
    differences: list[dict[str, Any]] = []
    matched = 0

    for name in names:
        left_result = left_map.get(name)
        right_result = right_map.get(name)
        if left_result is None or right_result is None:
            differences.append(
                {
                    "name": name,
                    "kind": "missing_left" if left_result is None else "missing_right",
                    "left": left_result,
                    "right": right_result,
                }
            )
            continue

        fields: list[dict[str, Any]] = []
        for field in COMPARE_FIELDS:
            left_value = left_result.get(field)
            right_value = right_result.get(field)
            if not _json_equal(left_value, right_value):
                fields.append({"field": field, "left": left_value, "right": right_value})
        if fields:
            differences.append({"name": name, "kind": "different", "fields": fields})
        else:
            matched += 1

    total = len(names)
    missing_left = sum(1 for item in differences if item["kind"] == "missing_left")
    missing_right = sum(1 for item in differences if item["kind"] == "missing_right")
    return {
        "left_impl": left.get("impl_name"),
        "right_impl": right.get("impl_name"),
        "summary": {
            "total": total,
            "matched": matched,
            "different": len(differences),
            "missing_left": missing_left,
            "missing_right": missing_right,
        },
        "differences": differences,
    }


def _display(value: Any) -> str:
    """Render a JSON value compactly for the text report."""
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def format_report(report: dict[str, Any], left_path: Path, right_path: Path) -> str:
    """Format a comparison report for terminal users."""
    summary = report["summary"]
    lines = [
        "VM differential comparison",
        f"  left : {left_path} ({report.get('left_impl') or 'unknown implementation'})",
        f"  right: {right_path} ({report.get('right_impl') or 'unknown implementation'})",
        (
            "  vectors: {total} total, {matched} matched, {different} different"
            " (missing left: {missing_left}, missing right: {missing_right})"
        ).format(**summary),
    ]
    differences = report["differences"]
    if not differences:
        lines.append("  result: PASS (all vectors match)")
        return "\n".join(lines)

    lines.append(f"  result: FAIL ({len(differences)} vector(s) differ)")
    for difference in differences:
        name = difference["name"]
        kind = difference["kind"]
        if kind != "different":
            present_side = "right" if kind == "missing_left" else "left"
            lines.append(f"\n  - {name}: missing from {'left' if kind == 'missing_left' else 'right'}; present in {present_side}")
            continue
        lines.append(f"\n  - {name}:")
        for field_difference in difference["fields"]:
            lines.append(
                f"      {field_difference['field']}: "
                f"left={_display(field_difference['left'])} "
                f"right={_display(field_difference['right'])}"
            )
    return "\n".join(lines)


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Strictly compare two NeoVM runner JSON outputs.",
        epilog="exit codes: 0=match, 1=difference, 2=input/usage error",
    )
    parser.add_argument("left", type=Path, help="first runner output JSON (for example Rust)")
    parser.add_argument("right", type=Path, help="second runner output JSON (for example C#)")
    parser.add_argument(
        "--report",
        type=Path,
        metavar="PATH",
        help="write the machine-readable comparison report to PATH",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="print the machine-readable report instead of the human-readable report",
    )
    parser.add_argument("--quiet", action="store_true", help="suppress the terminal report")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """Run the comparator and return its process exit code."""
    parser = _build_parser()
    try:
        args = parser.parse_args(argv)
    except SystemExit as exc:
        # argparse uses status 2 for usage errors and 0 for --help.
        return int(exc.code)

    try:
        left = _load_json(args.left)
        right = _load_json(args.right)
        report = compare_results(left, right)
        if args.report is not None:
            try:
                args.report.parent.mkdir(parents=True, exist_ok=True)
                args.report.write_text(
                    json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
                )
            except (OSError, UnicodeError) as exc:
                raise InputError(f"cannot write report {args.report}: {exc}") from exc
    except InputError as exc:
        print(f"vm-diff: input error: {exc}", file=sys.stderr)
        return 2

    if not args.quiet:
        if args.json:
            print(json.dumps(report, indent=2, ensure_ascii=False))
        else:
            print(format_report(report, args.left, args.right))
    return 0 if not report["differences"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
