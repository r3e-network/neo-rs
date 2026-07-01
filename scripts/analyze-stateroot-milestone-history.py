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
DEFAULT_MIN_TRANSACTION_BLOCKS = 1000


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
                    "speed_proof_source": milestone.get("speed_proof_source"),
                    "import_window_blocks_per_second": milestone.get(
                        "import_window_blocks_per_second"
                    ),
                    "replay_window_blocks_per_second": milestone.get(
                        "replay_window_blocks_per_second"
                    ),
                    "empty_block_speed_proof_source": milestone.get(
                        "empty_block_speed_proof_source"
                    ),
                    "empty_block_blocks_per_second": milestone.get(
                        "empty_block_blocks_per_second"
                    ),
                    "empty_only_blocks": milestone.get("empty_only_blocks"),
                    "empty_block_import_seconds": milestone.get(
                        "empty_block_import_seconds"
                    ),
                    "empty_block_speed_proof_error": milestone.get(
                        "empty_block_speed_proof_error"
                    ),
                    "transaction_work_summary": milestone.get(
                        "transaction_work_summary"
                    )
                    or {},
                    "sync_proof": milestone.get("sync_proof") or {},
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


def checkpoint_restore_metadata_reason(path: Path, height: int) -> str | None:
    if metadata_value(path, "restore_verified") != "true":
        return "missing restore_verified=true"
    verified_height = metadata_value(path, "verified_height")
    if verified_height != str(height):
        return (
            "verified_height does not match checkpoint height: "
            f"verified_height={verified_height}, height={height}"
        )
    if not metadata_value(path, "verified_stateroot_root"):
        return "missing verified_stateroot_root"
    if metadata_value(path, "verified_against_reference") != "true":
        return "missing verified_against_reference=true"
    return None


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
        restore_metadata_reason = (
            checkpoint_restore_metadata_reason(path, height)
            if has_chain and has_stateroot
            else None
        )
        usable_for_state_validation = bool(
            has_chain and has_stateroot and restore_metadata_reason is None
        )
        checkpoints.append(
            {
                "path": str(path),
                "height": height,
                "has_chain": has_chain,
                "has_stateroot": has_stateroot,
                "restore_metadata_reason": restore_metadata_reason,
                "usable_for_state_validation": usable_for_state_validation,
            }
        )

    checkpoints.sort(key=lambda item: int(item["height"]))
    full_state = [item for item in checkpoints if item["usable_for_state_validation"]]
    chain_only = [item for item in checkpoints if item["has_chain"] and not item["has_stateroot"]]
    structural_not_verified = [
        item
        for item in checkpoints
        if item["has_chain"]
        and item["has_stateroot"]
        and not item["usable_for_state_validation"]
    ]
    latest = full_state[-1] if full_state else None
    return {
        "root": str(root),
        "exists": True,
        "total_count": len(checkpoints),
        "full_state_count": len(full_state),
        "chain_only_count": len(chain_only),
        "structural_not_restore_verified_count": len(structural_not_verified),
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
        "structural_not_restore_verified_heights": [
            item["height"] for item in structural_not_verified
        ],
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


def fast_sync_import_report(item: dict[str, Any]) -> dict[str, Any]:
    sync_proof = item.get("sync_proof") or {}
    if not isinstance(sync_proof, dict):
        return {}
    import_report = sync_proof.get("fast_sync_import") or {}
    if not isinstance(import_report, dict):
        return {}
    return import_report


def transaction_import_proof_milestones(
    completed: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    proofs: list[dict[str, Any]] = []
    for item in completed:
        if item.get("speed_proof_source") != "fast-sync-transaction-blocks":
            continue
        if item.get("import_window_blocks_per_second") is None:
            continue
        try:
            bps = float(item["import_window_blocks_per_second"])
        except (TypeError, ValueError):
            continue
        import_report = fast_sync_import_report(item)
        proof = {
            "run_index": item.get("run_index"),
            "timestamp_utc": item.get("timestamp_utc"),
            "node_bin": item.get("node_bin"),
            "probe_bin": item.get("probe_bin"),
            "height": milestone_height(item),
            "local_root": item.get("local_root"),
            "transaction_import_blocks_per_second": bps,
            "transaction_blocks": import_report.get("transaction_blocks"),
            "transactions": import_report.get("transactions"),
            "transaction_block_import_seconds": import_report.get(
                "transaction_block_import_seconds"
            ),
            "replay_window_blocks_per_second": item.get("replay_window_blocks_per_second"),
        }
        proofs.append(proof)
    return proofs


def transaction_import_floor_violations(
    proofs: list[dict[str, Any]],
    *,
    sync_speed_floor_bps: float,
) -> list[dict[str, Any]]:
    violations: list[dict[str, Any]] = []
    for proof in proofs:
        bps = float(proof.get("transaction_import_blocks_per_second") or 0.0)
        if bps >= sync_speed_floor_bps:
            continue
        violation = dict(proof)
        violation["shortfall_blocks_per_second"] = sync_speed_floor_bps - bps
        violations.append(violation)
    return violations


def summarize_transaction_import_proofs(
    proofs: list[dict[str, Any]],
    *,
    slowest_limit: int,
    fastest_limit: int,
    sync_speed_floor_bps: float,
) -> dict[str, Any]:
    bps_values = [
        float(item["transaction_import_blocks_per_second"]) for item in proofs
    ]
    violations = transaction_import_floor_violations(
        proofs,
        sync_speed_floor_bps=sync_speed_floor_bps,
    )
    return {
        "transaction_import_proof_count": len(proofs),
        "transaction_import_speed_floor_blocks_per_second": sync_speed_floor_bps,
        "average_transaction_import_blocks_per_second": (
            sum(bps_values) / len(bps_values) if bps_values else 0.0
        ),
        "min_transaction_import_blocks_per_second": (
            min(bps_values) if bps_values else 0.0
        ),
        "max_transaction_import_blocks_per_second": (
            max(bps_values) if bps_values else 0.0
        ),
        "transaction_import_floor_violation_count": len(violations),
        "transaction_import_floor_violations": violations,
        "slowest_transaction_import_milestones": sorted(
            proofs,
            key=lambda item: float(item["transaction_import_blocks_per_second"]),
        )[:slowest_limit],
        "fastest_transaction_import_milestones": sorted(
            proofs,
            key=lambda item: float(item["transaction_import_blocks_per_second"]),
            reverse=True,
        )[:fastest_limit],
    }


def empty_block_fast_path_milestones(
    completed: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    proofs: list[dict[str, Any]] = []
    for item in completed:
        if item.get("empty_block_speed_proof_source") != "fast-sync-empty-blocks":
            continue
        if item.get("empty_block_blocks_per_second") is None:
            continue
        try:
            bps = float(item["empty_block_blocks_per_second"])
        except (TypeError, ValueError):
            continue
        proofs.append(
            {
                "run_index": item.get("run_index"),
                "timestamp_utc": item.get("timestamp_utc"),
                "node_bin": item.get("node_bin"),
                "probe_bin": item.get("probe_bin"),
                "height": milestone_height(item),
                "local_root": item.get("local_root"),
                "empty_block_blocks_per_second": bps,
                "empty_only_blocks": item.get("empty_only_blocks"),
                "empty_block_import_seconds": item.get("empty_block_import_seconds"),
                "empty_block_speed_proof_error": item.get(
                    "empty_block_speed_proof_error"
                ),
                "transaction_import_blocks_per_second": item.get(
                    "import_window_blocks_per_second"
                ),
            }
        )
    return proofs


def summarize_empty_block_fast_path(
    proofs: list[dict[str, Any]],
    *,
    slowest_limit: int,
    fastest_limit: int,
    empty_block_speed_floor_bps: float | None,
) -> dict[str, Any]:
    bps_values = [float(item["empty_block_blocks_per_second"]) for item in proofs]
    proof_errors = [
        item for item in proofs if item.get("empty_block_speed_proof_error")
    ]
    floor_violations: list[dict[str, Any]] = []
    if empty_block_speed_floor_bps is not None:
        for proof in proofs:
            bps = float(proof["empty_block_blocks_per_second"])
            if bps >= empty_block_speed_floor_bps:
                continue
            violation = dict(proof)
            violation["shortfall_blocks_per_second"] = empty_block_speed_floor_bps - bps
            floor_violations.append(violation)
    return {
        "empty_block_fast_path_proof_count": len(proofs),
        "empty_block_speed_floor_blocks_per_second": empty_block_speed_floor_bps,
        "average_empty_block_blocks_per_second": (
            sum(bps_values) / len(bps_values) if bps_values else 0.0
        ),
        "min_empty_block_blocks_per_second": min(bps_values) if bps_values else 0.0,
        "max_empty_block_blocks_per_second": max(bps_values) if bps_values else 0.0,
        "slowest_empty_block_milestones": sorted(
            proofs,
            key=lambda item: float(item["empty_block_blocks_per_second"]),
        )[:slowest_limit],
        "fastest_empty_block_milestones": sorted(
            proofs,
            key=lambda item: float(item["empty_block_blocks_per_second"]),
            reverse=True,
        )[:fastest_limit],
        "empty_block_speed_floor_violation_count": len(floor_violations),
        "empty_block_speed_floor_violations": floor_violations,
        "empty_block_speed_proof_error_count": len(proof_errors),
        "empty_block_speed_proof_errors": proof_errors,
    }


def transaction_import_sample_size_violations(
    proofs: list[dict[str, Any]],
    *,
    minimum_transaction_blocks: int,
) -> list[dict[str, Any]]:
    violations: list[dict[str, Any]] = []
    for proof in proofs:
        try:
            transaction_blocks = int(proof.get("transaction_blocks") or 0)
        except (TypeError, ValueError):
            transaction_blocks = 0
        if transaction_blocks >= minimum_transaction_blocks:
            continue
        violation = dict(proof)
        violation["minimum_transaction_blocks"] = minimum_transaction_blocks
        violation["missing_transaction_blocks"] = (
            minimum_transaction_blocks - transaction_blocks
        )
        violations.append(violation)
    return violations


def production_proof_readiness(
    *,
    reference_mismatch_count: int,
    state_mismatch_count: int,
    transaction_import_proofs: list[dict[str, Any]],
    transaction_import_floor_violations: list[dict[str, Any]],
    checkpoint_inventory: dict[str, Any] | None,
    minimum_transaction_blocks: int = DEFAULT_MIN_TRANSACTION_BLOCKS,
    minimum_full_state_checkpoints: int = DEFAULT_MIN_FULL_STATE_CHECKPOINTS,
) -> dict[str, Any]:
    sample_size_violations = transaction_import_sample_size_violations(
        transaction_import_proofs,
        minimum_transaction_blocks=minimum_transaction_blocks,
    )
    full_state_checkpoint_count = (
        int(checkpoint_inventory.get("full_state_count") or 0)
        if checkpoint_inventory
        else 0
    )
    checkpoint_floor_met = (
        full_state_checkpoint_count >= minimum_full_state_checkpoints
    )
    blocking_reasons: list[str] = []
    if state_mismatch_count:
        blocking_reasons.append("state root mismatch exists")
    if reference_mismatch_count:
        blocking_reasons.append("reference state root mismatch exists")
    if not transaction_import_proofs:
        blocking_reasons.append("missing transaction-bearing import speed proof")
    if transaction_import_floor_violations:
        blocking_reasons.append(
            "transaction-bearing import speed is below configured floor"
        )
    if sample_size_violations:
        blocking_reasons.append(
            f"transaction import proof has fewer than {minimum_transaction_blocks} "
            "transaction-bearing blocks"
        )
    if not checkpoint_floor_met:
        blocking_reasons.append(
            f"fewer than {minimum_full_state_checkpoints} restore-verified "
            "full-state checkpoints retained"
        )

    return {
        "ready": not blocking_reasons,
        "blocking_reasons": blocking_reasons,
        "minimum_transaction_blocks": minimum_transaction_blocks,
        "minimum_full_state_checkpoints": minimum_full_state_checkpoints,
        "state_roots_match_chain": state_mismatch_count == 0,
        "references_match_local": reference_mismatch_count == 0,
        "transaction_import_proof_count": len(transaction_import_proofs),
        "transaction_import_speed_floor_met": not transaction_import_floor_violations
        and bool(transaction_import_proofs),
        "transaction_import_sample_size_met": not sample_size_violations
        and bool(transaction_import_proofs),
        "transaction_import_sample_size_violation_count": len(sample_size_violations),
        "transaction_import_sample_size_violations": sample_size_violations,
        "restore_verified_checkpoint_count": full_state_checkpoint_count,
        "restore_verified_checkpoint_floor_met": checkpoint_floor_met,
    }


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
    empty_block_speed_floor_bps: float | None = None,
    minimum_transaction_blocks: int = DEFAULT_MIN_TRANSACTION_BLOCKS,
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
    transaction_import_proofs = transaction_import_proof_milestones(completed)
    transaction_import_floor_shortfalls = transaction_import_floor_violations(
        transaction_import_proofs,
        sync_speed_floor_bps=sync_speed_floor_bps,
    )
    transaction_import_summary = summarize_transaction_import_proofs(
        transaction_import_proofs,
        slowest_limit=slowest_limit,
        fastest_limit=fastest_limit,
        sync_speed_floor_bps=sync_speed_floor_bps,
    )
    empty_block_proofs = empty_block_fast_path_milestones(completed)
    empty_block_summary = summarize_empty_block_fast_path(
        empty_block_proofs,
        slowest_limit=slowest_limit,
        fastest_limit=fastest_limit,
        empty_block_speed_floor_bps=empty_block_speed_floor_bps,
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
        "latest_transaction_work_summary": (
            latest.get("transaction_work_summary") if latest else {}
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
    report.update(transaction_import_summary)
    report.update(empty_block_summary)
    checkpoint_inventory_report = None
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
        checkpoint_inventory_report = inventory
        report["checkpoint_inventory"] = inventory
    report["production_proof_readiness"] = production_proof_readiness(
        reference_mismatch_count=len(reference_mismatches),
        state_mismatch_count=len(state_mismatches),
        transaction_import_proofs=transaction_import_proofs,
        transaction_import_floor_violations=transaction_import_floor_shortfalls,
        checkpoint_inventory=checkpoint_inventory_report,
        minimum_transaction_blocks=minimum_transaction_blocks,
    )
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
    parser.add_argument(
        "--empty-block-speed-floor-bps",
        type=float,
        default=None,
        help=(
            "Optionally flag fast-sync empty-block fast-path proofs below this "
            "blocks/second floor. Empty-block speed is reported separately from "
            "transaction-bearing speed and has no default cap or target."
        ),
    )
    parser.add_argument(
        "--minimum-transaction-blocks",
        type=int,
        default=DEFAULT_MIN_TRANSACTION_BLOCKS,
        help=(
            "Minimum transaction-bearing blocks required before a fast-sync "
            "transaction import speed proof can satisfy production readiness."
        ),
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
            empty_block_speed_floor_bps=args.empty_block_speed_floor_bps,
            minimum_transaction_blocks=args.minimum_transaction_blocks,
        )
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
