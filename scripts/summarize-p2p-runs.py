#!/usr/bin/env python3
"""Summarize completed NEO_LIVE_STOP_HEIGHT P2P audit runs from JSON logs."""

from __future__ import annotations

import argparse
from datetime import datetime
import json
from pathlib import Path
import statistics


def timestamp(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("logs", type=Path, nargs="+")
    parser.add_argument("--min-blocks", type=int, default=1)
    args = parser.parse_args()

    runs = []
    for path in args.logs:
        started = None
        start_height = None
        stopped = None
        stop_height = None
        for line in path.read_text(encoding="utf-8").splitlines():
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            fields = record.get("fields", {})
            message = fields.get("message", "")
            if message == "Node starting, broadcasting NodeStarted event" and started is None:
                started = timestamp(record["timestamp"])
            if message == "block index request - pipeline empty" and start_height is None:
                start_height = fields.get("current_height")
            if message == "reached NEO_LIVE_STOP_HEIGHT; shutting down through the normal flush path":
                stopped = timestamp(record["timestamp"])
                stop_height = fields.get("current_height")
        if started is None or stopped is None or start_height is None or stop_height is None:
            continue
        blocks = stop_height - start_height
        elapsed = (stopped - started).total_seconds()
        if blocks < args.min_blocks or elapsed <= 0:
            continue
        runs.append((path, start_height, stop_height, blocks, elapsed, blocks / elapsed))

    runs.sort(key=lambda run: run[1])
    for path, start, stop, blocks, elapsed, rate in runs:
        print(
            f"{path}: start={start} stop={stop} blocks={blocks} "
            f"elapsed_s={elapsed:.3f} blocks_per_s={rate:.3f}"
        )
    if runs:
        elapsed_values = sorted(run[4] for run in runs)
        rate_values = sorted(run[5] for run in runs)
        p95_index = min(len(elapsed_values) - 1, int(len(elapsed_values) * 0.95))
        print(
            f"summary: runs={len(runs)} elapsed_min_s={elapsed_values[0]:.3f} "
            f"elapsed_p50_s={statistics.median(elapsed_values):.3f} "
            f"elapsed_p95_s={elapsed_values[p95_index]:.3f} "
            f"elapsed_max_s={elapsed_values[-1]:.3f} "
            f"rate_min={rate_values[0]:.3f} rate_median={statistics.median(rate_values):.3f} "
            f"rate_max={rate_values[-1]:.3f}"
        )


if __name__ == "__main__":
    main()
