#!/usr/bin/env python3
"""Build a dry-run plan for the MainNet state-root validation stack."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from mainnet_validation_stack import (
    DEFAULT_CHECKPOINT_WAITING_INTERVAL,
    DEFAULT_CHECKPOINT_WATCH_INTERVAL,
    DEFAULT_LOG_DIR,
    DEFAULT_NODE_BIN,
    DEFAULT_NODE_CONFIG,
    DEFAULT_RESUME_FILE,
    DEFAULT_STATUS_FILE,
    build_plan,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Print a dry-run plan for node + state-root validator + checkpoint maintenance."
    )
    parser.add_argument("--node-config", default=DEFAULT_NODE_CONFIG)
    parser.add_argument("--node-bin", default=DEFAULT_NODE_BIN)
    parser.add_argument("--status-file", default=DEFAULT_STATUS_FILE)
    parser.add_argument("--resume-file", default=DEFAULT_RESUME_FILE)
    parser.add_argument("--log-dir", default=DEFAULT_LOG_DIR)
    parser.add_argument("--batch", type=int, default=500)
    parser.add_argument("--poll-interval", type=int, default=5)
    parser.add_argument(
        "--checkpoint-watch-interval",
        type=int,
        default=DEFAULT_CHECKPOINT_WATCH_INTERVAL,
    )
    parser.add_argument(
        "--checkpoint-waiting-interval",
        type=int,
        default=DEFAULT_CHECKPOINT_WAITING_INTERVAL,
    )
    parser.add_argument(
        "--checkpoint-execute",
        action="store_true",
        help="Include --execute in the checkpoint-maintainer command.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    plan = build_plan(
        node_config=Path(args.node_config),
        node_bin=Path(args.node_bin),
        status_file=Path(args.status_file),
        resume_file=Path(args.resume_file),
        log_dir=Path(args.log_dir),
        batch=args.batch,
        poll_interval=args.poll_interval,
        checkpoint_watch_interval=args.checkpoint_watch_interval,
        checkpoint_waiting_interval=args.checkpoint_waiting_interval,
        checkpoint_execute=args.checkpoint_execute,
    )
    print(json.dumps(plan, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
