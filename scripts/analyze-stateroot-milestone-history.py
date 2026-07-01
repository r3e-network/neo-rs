#!/usr/bin/env python3
"""Analyze StateRoot milestone summary history JSONL."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

DEFAULT_SYNC_SPEED_FLOOR_BPS = 1500.0
DEFAULT_MIN_FULL_STATE_CHECKPOINTS = 3


def load_history(path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"{path}:{line_number}: invalid JSON: {exc}") from exc
            if not isinstance(record, dict):
                raise ValueError(f"{path}:{line_number}: expected JSON object")
            records.append(record)
    return records


def flatten_milestones(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    flattened: list[dict[str, Any]] = []
    for run_index, record in enumerate(records):
        summary = record.get("summary") or {}
        for milestone in summary.get("milestones") or []:
            if not isinstance(milestone, dict):
                continue
            flattened.append(
                {
                    "run_index": run_index,
                    "timestamp_utc": record.get("timestamp_utc"),
                    "config": record.get("config"),
                    "node_bin": record.get("node_bin") or "unknown",
                    "probe_bin": record.get("probe_bin") or "unknown",
                    "height": milestone.get("height"),
                    "last_height": milestone.get("last_height"),
                    "blocks_per_second": milestone.get("blocks_per_second", 0.0),
                    "elapsed_seconds": milestone.get("elapsed_seconds", 0.0),
                    "local_root": milestone.get("local_root"),
                    "reference_matches_local": milestone.get("reference_matches_local"),
                    "stateroot_matches_chain": milestone.get("stateroot_matches_chain"),
                    "checkpoint_created": milestone.get("checkpoint_created"),
                    "successful_reference_samples": milestone.get(
                        "successful_reference_samples", 0
                    ),
                    "height_sample_rate_summary": milestone.get(
                        "height_sample_rate_summary"
                    )
                    or {},
                    "metrics_sample_summary": milestone.get("metrics_sample_summary") or {},
                }
            )
    return flattened


def sorted_numeric(values: list[dict[str, Any]], key: str, *, reverse: bool) -> list[dict[str, Any]]:
    return sorted(
        values,
        key=lambda item: float(item.get(key) or 0.0),
        reverse=reverse,
    )


def metadata_value(path: Path, key: str) -> str | None:
    info = path / "CHECKPOINT_INFO"
    if not info.exists():
        return None
    for line in info.read_text(encoding="utf-8").splitlines():
        if line.startswith(f"{key}="):
            return line.split("=", 1)[1]
    return None


def checkpoint_height(path: Path) -> int | None:
    if path.name.startswith("h") and path.name[1:].isdigit():
        return int(path.name[1:])
    return None


def checkpoint_has_chain(path: Path) -> bool:
    return (path / "mainnet").is_dir()


def checkpoint_has_stateroot(path: Path) -> bool:
    if not (path / "StateRoot").is_dir():
        return False
    if metadata_value(path, "state_root_included") == "false":
        return False
    return True


def scan_checkpoint_inventory(root: Path) -> dict[str, Any]:
    if not root.is_dir():
        return {
            "root": str(root),
            "exists": False,
            "total_count": 0,
            "full_state_count": 0,
            "chain_only_count": 0,
            "latest_full_state_height": None,
            "latest_full_state_path": None,
            "retained_heights": [],
            "full_state_heights": [],
            "chain_only_heights": [],
        }

    checkpoints: list[dict[str, Any]] = []
    for path in sorted(item for item in root.iterdir() if item.is_dir()):
        height = checkpoint_height(path)
        if height is None:
            continue
        has_chain = checkpoint_has_chain(path)
        has_stateroot = checkpoint_has_stateroot(path)
        checkpoints.append(
            {
                "path": str(path),
                "height": height,
                "has_chain": has_chain,
                "has_stateroot": has_stateroot,
                "usable_for_state_validation": bool(has_chain and has_stateroot),
            }
        )

    checkpoints.sort(key=lambda item: int(item["height"]))
    full_state = [item for item in checkpoints if item["usable_for_state_validation"]]
    chain_only = [item for item in checkpoints if item["has_chain"] and not item["has_stateroot"]]
    latest = full_state[-1] if full_state else None
    return {
        "root": str(root),
        "exists": True,
        "total_count": len(checkpoints),
        "full_state_count": len(full_state),
        "chain_only_count": len(chain_only),
        "minimum_full_state_checkpoints": DEFAULT_MIN_FULL_STATE_CHECKPOINTS,
        "minimum_full_state_checkpoints_met": len(full_state)
        >= DEFAULT_MIN_FULL_STATE_CHECKPOINTS,
        "missing_full_state_checkpoint_count": max(
            0,
            DEFAULT_MIN_FULL_STATE_CHECKPOINTS - len(full_state),
        ),
        "latest_full_state_height": latest["height"] if latest else None,
        "latest_full_state_path": latest["path"] if latest else None,
        "retained_heights": [item["height"] for item in checkpoints],
        "full_state_heights": [item["height"] for item in full_state],
        "chain_only_heights": [item["height"] for item in chain_only],
    }


def milestone_height(item: dict[str, Any]) -> int | None:
    value = item.get("last_height") or item.get("height")
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def completed_milestones_in_history_order(milestones: list[dict[str, Any]]) -> list[dict[str, Any]]:
    completed = [item for item in milestones if item.get("checkpoint_created")]
    return sorted(
        completed,
        key=lambda item: (
            int(item.get("run_index") or 0),
            milestone_height(item) if milestone_height(item) is not None else -1,
        ),
    )


def throughput_trend(
    completed: list[dict[str, Any]],
    *,
    regression_threshold_percent: float,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    trend: list[dict[str, Any]] = []
    regressions: list[dict[str, Any]] = []
    previous: dict[str, Any] | None = None
    for item in completed:
        height = milestone_height(item)
        bps = float(item.get("blocks_per_second") or 0.0)
        previous_height = milestone_height(previous) if previous else None
        previous_bps = (
            float(previous.get("blocks_per_second") or 0.0)
            if previous is not None
            else None
        )
        change_percent = None
        regression = False
        if previous_bps is not None and previous_bps > 0:
            change_percent = ((bps - previous_bps) / previous_bps) * 100.0
            regression = change_percent <= -abs(regression_threshold_percent)
        entry = {
            "run_index": item.get("run_index"),
            "timestamp_utc": item.get("timestamp_utc"),
            "node_bin": item.get("node_bin"),
            "probe_bin": item.get("probe_bin"),
            "height": height,
            "previous_height": previous_height,
            "height_delta": (
                height - previous_height
                if height is not None and previous_height is not None
                else None
            ),
            "blocks_per_second": bps,
            "previous_blocks_per_second": previous_bps,
            "change_percent": change_percent,
            "regression": regression,
            "local_root": item.get("local_root"),
        }
        trend.append(entry)
        if regression:
            regressions.append(entry)
        previous = item
    return trend, regressions


def performance_by_node_bin(completed: list[dict[str, Any]]) -> list[dict[str, Any]]:
    groups: dict[str, list[dict[str, Any]]] = {}
    for item in completed:
        groups.setdefault(str(item.get("node_bin") or "unknown"), []).append(item)

    summaries: list[dict[str, Any]] = []
    for node_bin, items in groups.items():
        bps_values = [float(item.get("blocks_per_second") or 0.0) for item in items]
        heights = [
            height
            for height in (milestone_height(item) for item in items)
            if height is not None
        ]
        latest = max(items, key=lambda item: milestone_height(item) or -1, default=None)
        probe_bins = sorted({str(item.get("probe_bin") or "unknown") for item in items})
        summaries.append(
            {
                "node_bin": node_bin,
                "probe_bins": probe_bins,
                "milestone_count": len(items),
                "height_min": min(heights) if heights else None,
                "height_max": max(heights) if heights else None,
                "latest_height": milestone_height(latest) if latest else None,
                "latest_root": latest.get("local_root") if latest else None,
                "average_blocks_per_second": (
                    sum(bps_values) / len(bps_values) if bps_values else 0.0
                ),
                "min_blocks_per_second": min(bps_values) if bps_values else 0.0,
                "max_blocks_per_second": max(bps_values) if bps_values else 0.0,
            }
        )
    return sorted(
        summaries,
        key=lambda item: (
            item["node_bin"] == "unknown",
            str(item["node_bin"]),
        ),
    )


def throughput_floor_violations(
    completed: list[dict[str, Any]],
    *,
    sync_speed_floor_bps: float,
) -> list[dict[str, Any]]:
    violations: list[dict[str, Any]] = []
    for item in completed:
        bps = float(item.get("blocks_per_second") or 0.0)
        if bps >= sync_speed_floor_bps:
            continue
        height = milestone_height(item)
        violations.append(
            {
                "run_index": item.get("run_index"),
                "timestamp_utc": item.get("timestamp_utc"),
                "node_bin": item.get("node_bin"),
                "probe_bin": item.get("probe_bin"),
                "height": height,
                "blocks_per_second": bps,
                "shortfall_blocks_per_second": sync_speed_floor_bps - bps,
                "local_root": item.get("local_root"),
            }
        )
    return violations


def sample_interval_rankings(
    completed: list[dict[str, Any]],
    *,
    limit: int,
    fastest: bool,
) -> list[dict[str, Any]]:
    intervals: list[dict[str, Any]] = []
    interval_key = "fastest_interval" if fastest else "slowest_interval"
    for item in completed:
        summary = item.get("height_sample_rate_summary") or {}
        interval = summary.get(interval_key)
        if not isinstance(interval, dict):
            continue
        if int(interval.get("height_delta") or 0) <= 0:
            continue
        intervals.append(
            {
                "run_index": item.get("run_index"),
                "timestamp_utc": item.get("timestamp_utc"),
                "node_bin": item.get("node_bin"),
                "height": milestone_height(item),
                "local_root": item.get("local_root"),
                **interval,
            }
        )
    return sorted(
        intervals,
        key=lambda item: float(item.get("blocks_per_second") or 0.0),
        reverse=fastest,
    )[:limit]


def hot_metrics_by_average_us(
    completed: list[dict[str, Any]],
    *,
    limit: int,
) -> list[dict[str, Any]]:
    groups: dict[str, dict[str, Any]] = {}
    for item in completed:
        summary = item.get("metrics_sample_summary") or {}
        hot_metrics = summary.get("hot_metrics_by_average_us") or []
        if not isinstance(hot_metrics, list):
            continue
        height = milestone_height(item)
        for metric in hot_metrics:
            if not isinstance(metric, dict):
                continue
            name = metric.get("name")
            if not name:
                continue
            try:
                average_us = float(metric.get("average_us"))
                sample_count = int(metric.get("sample_count") or 0)
            except (TypeError, ValueError):
                continue
            if sample_count <= 0:
                continue
            group = groups.setdefault(
                str(name),
                {
                    "weighted_total_us": 0.0,
                    "sample_count": 0,
                    "milestone_count": 0,
                    "max_us": 0.0,
                    "heights": [],
                },
            )
            group["weighted_total_us"] += average_us * sample_count
            group["sample_count"] += sample_count
            group["milestone_count"] += 1
            if height is not None:
                group["heights"].append(height)
            try:
                group["max_us"] = max(float(group["max_us"]), float(metric.get("max_us")))
            except (TypeError, ValueError):
                group["max_us"] = max(float(group["max_us"]), average_us)

    ranked = [
        {
            "name": name,
            "average_us": group["weighted_total_us"] / group["sample_count"],
            "max_us": group["max_us"],
            "sample_count": group["sample_count"],
            "milestone_count": group["milestone_count"],
            "heights": group["heights"],
        }
        for name, group in groups.items()
        if group["sample_count"] > 0
    ]
    ranked.sort(key=lambda item: (-float(item["average_us"]), item["name"]))
    return ranked[:limit]


def analyze_history(
    records: list[dict[str, Any]],
    *,
    slowest_limit: int,
    fastest_limit: int,
    checkpoint_root: Path | None = None,
    regression_threshold_percent: float = 25.0,
    sync_speed_floor_bps: float = DEFAULT_SYNC_SPEED_FLOOR_BPS,
) -> dict:
    milestones = flatten_milestones(records)
    completed = completed_milestones_in_history_order(milestones)
    reference_mismatches = [
        item for item in milestones if item.get("reference_matches_local") is not True
    ]
    state_mismatches = [
        item for item in milestones if item.get("stateroot_matches_chain") is not True
    ]
    bps_values = [
        float(item["blocks_per_second"])
        for item in milestones
        if item.get("blocks_per_second") is not None
    ]
    latest = max(
        completed,
        key=lambda item: int(item.get("last_height") or item.get("height") or -1),
        default=None,
    )
    trend, regressions = throughput_trend(
        completed,
        regression_threshold_percent=regression_threshold_percent,
    )
    floor_violations = throughput_floor_violations(
        completed,
        sync_speed_floor_bps=sync_speed_floor_bps,
    )
    report: dict[str, Any] = {
        "run_count": len(records),
        "milestone_count": len(milestones),
        "completed_checkpoint_count": len(completed),
        "latest_height": latest.get("last_height") if latest else None,
        "latest_root": latest.get("local_root") if latest else None,
        "latest_metrics_sample_summary": (
            latest.get("metrics_sample_summary") if latest else {}
        )
        or {},
        "average_blocks_per_second": sum(bps_values) / len(bps_values) if bps_values else 0.0,
        "slowest_milestones": sorted_numeric(
            milestones,
            "blocks_per_second",
            reverse=False,
        )[:slowest_limit],
        "fastest_milestones": sorted_numeric(
            milestones,
            "blocks_per_second",
            reverse=True,
        )[:fastest_limit],
        "reference_mismatch_count": len(reference_mismatches),
        "state_mismatch_count": len(state_mismatches),
        "throughput_regression_threshold_percent": regression_threshold_percent,
        "throughput_trend": trend,
        "throughput_regression_count": len(regressions),
        "throughput_regressions": regressions,
        "sync_speed_floor_blocks_per_second": sync_speed_floor_bps,
        "throughput_floor_violation_count": len(floor_violations),
        "throughput_floor_violations": floor_violations,
        "performance_by_node_bin": performance_by_node_bin(completed),
        "slowest_sample_intervals": sample_interval_rankings(
            completed,
            limit=slowest_limit,
            fastest=False,
        ),
        "fastest_sample_intervals": sample_interval_rankings(
            completed,
            limit=fastest_limit,
            fastest=True,
        ),
        "hot_metrics_by_average_us": hot_metrics_by_average_us(
            completed,
            limit=slowest_limit,
        ),
        "reference_mismatches": reference_mismatches,
        "state_mismatches": state_mismatches,
    }
    if checkpoint_root is not None:
        inventory = scan_checkpoint_inventory(checkpoint_root)
        history_heights = sorted(
            {
                height
                for height in (milestone_height(item) for item in completed)
                if height is not None
            }
        )
        retained_full_state = set(inventory["full_state_heights"])
        inventory["history_checkpoint_heights"] = history_heights
        inventory["history_checkpoints_not_retained"] = [
            height for height in history_heights if height not in retained_full_state
        ]
        inventory["retained_checkpoints_not_in_history"] = [
            height for height in inventory["full_state_heights"] if height not in history_heights
        ]
        report["checkpoint_inventory"] = inventory
    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Summarize StateRoot milestone JSONL history for performance trends."
    )
    parser.add_argument("history", type=Path)
    parser.add_argument(
        "--checkpoint-root",
        type=Path,
        default=None,
        help="Optional checkpoint root to report the currently retained full-state inventory.",
    )
    parser.add_argument("--slowest", type=int, default=5)
    parser.add_argument("--fastest", type=int, default=5)
    parser.add_argument(
        "--regression-threshold-percent",
        type=float,
        default=25.0,
        help="Flag adjacent milestone throughput drops at or above this percentage.",
    )
    parser.add_argument(
        "--sync-speed-floor-bps",
        type=float,
        default=DEFAULT_SYNC_SPEED_FLOOR_BPS,
        help="Flag completed checkpoint milestones below this blocks/second floor.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        records = load_history(args.history)
        report = analyze_history(
            records,
            slowest_limit=args.slowest,
            fastest_limit=args.fastest,
            checkpoint_root=args.checkpoint_root,
            regression_threshold_percent=args.regression_threshold_percent,
            sync_speed_floor_bps=args.sync_speed_floor_bps,
        )
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
