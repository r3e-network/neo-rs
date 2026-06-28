"""Status-file helpers for continuous state-root validation."""

from __future__ import annotations

import json
import os
import time
from argparse import Namespace
from datetime import datetime
from pathlib import Path
from tempfile import NamedTemporaryFile
from typing import Any


def timestamp() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")


def atomic_write(path: str | None, payload: str) -> None:
    if not path:
        return
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp_path: str | None = None
    try:
        with NamedTemporaryFile(
            "w",
            dir=target.parent,
            encoding="utf-8",
            delete=False,
        ) as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
            tmp_path = handle.name
        os.replace(tmp_path, target)
    finally:
        if tmp_path and os.path.exists(tmp_path):
            os.unlink(tmp_path)


def save_json(path: str | None, payload: dict) -> None:
    atomic_write(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def build_status_payload(
    *,
    local_endpoint: Any,
    reference_endpoints: list[Any],
    start_block: int,
    next_block: int,
    last_validated_block: int,
    total_compared: int,
    total_matched: int,
    total_mismatched: int,
    total_errors: int,
    local_state_height: int | None,
    local_validated_height: int | None,
    local_block_count: int | None,
    mismatches: list[dict],
    errors: list[dict],
    started_at: float,
    status: str,
    target_stop_at: int | None,
) -> dict:
    elapsed = max(time.time() - started_at, 0.0)
    rate = total_compared / elapsed if elapsed > 0 else 0.0
    checkpoint_stages = build_checkpoint_stages(
        start_block=start_block,
        last_validated_block=last_validated_block,
    )
    return {
        "timestamp": timestamp(),
        "status": status,
        "local_url": local_endpoint.url,
        "reference_urls": [endpoint.url for endpoint in reference_endpoints],
        "start_block": start_block,
        "target_stop_at": target_stop_at,
        "next_block": next_block,
        "last_validated_block": last_validated_block,
        "local_state_height": local_state_height,
        "local_validated_height": local_validated_height,
        "local_block_count": local_block_count,
        "total_compared": total_compared,
        "total_matched": total_matched,
        "total_mismatched": total_mismatched,
        "total_errors": total_errors,
        "match_percentage": (total_matched / total_compared * 100.0)
        if total_compared
        else 0.0,
        "rate_per_second": rate,
        "elapsed_seconds": elapsed,
        "checkpoint_stages": checkpoint_stages,
        "recent_mismatches": mismatches,
        "recent_errors": errors,
    }


def build_checkpoint_stages(*, start_block: int, last_validated_block: int) -> list[dict]:
    """Return three validated-height recovery points for long mainnet syncs."""
    safe_start = max(start_block, 0)
    if last_validated_block < safe_start:
        return []
    safe_latest = max(last_validated_block, safe_start)
    safe_mid = safe_start + (safe_latest - safe_start) // 2
    return [
        checkpoint_stage("base", safe_start),
        checkpoint_stage("mid", safe_mid),
        checkpoint_stage("latest", safe_latest),
    ]


def checkpoint_stage(stage: str, height: int) -> dict:
    return {
        "stage": stage,
        "height": height,
        "label": f"{stage}-h{height}",
        "command": f"scripts/checkpoint-on-height.sh none --once --height {height}",
    }


def write_status(
    args: Namespace,
    local_endpoint: Any,
    reference_endpoints: list[Any],
    start_block: int,
    next_block: int,
    last_validated_block: int,
    total_compared: int,
    total_matched: int,
    total_mismatched: int,
    total_errors: int,
    local_state_height: int | None,
    local_validated_height: int | None,
    local_block_count: int | None,
    mismatches: list[dict],
    errors: list[dict],
    started_at: float,
    status: str,
    target_stop_at: int | None,
) -> None:
    save_json(
        args.status_file,
        build_status_payload(
            local_endpoint=local_endpoint,
            reference_endpoints=reference_endpoints,
            start_block=start_block,
            next_block=next_block,
            last_validated_block=last_validated_block,
            total_compared=total_compared,
            total_matched=total_matched,
            total_mismatched=total_mismatched,
            total_errors=total_errors,
            local_state_height=local_state_height,
            local_validated_height=local_validated_height,
            local_block_count=local_block_count,
            mismatches=mismatches,
            errors=errors,
            started_at=started_at,
            status=status,
            target_stop_at=target_stop_at,
        ),
    )
