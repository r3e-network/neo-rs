#!/usr/bin/env python3
"""Run or inspect the MainNet state-root validation process stack."""

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
    DEFAULT_PID_DIR,
    DEFAULT_RESUME_FILE,
    DEFAULT_STATUS_FILE,
    build_plan,
    stack_status,
    start_stack,
    stop_stack,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Start, stop, inspect, or dry-run the node + state-root validator + "
            "checkpoint maintainer stack."
        )
    )
    action = parser.add_mutually_exclusive_group()
    action.add_argument(
        "--start",
        action="store_true",
        help="Run preflight and start the three background processes.",
    )
    action.add_argument(
        "--status",
        action="store_true",
        help="Read PID files and report whether each process is running.",
    )
    action.add_argument(
        "--stop",
        action="store_true",
        help="Send SIGTERM to running PIDs in reverse stack order.",
    )
    parser.add_argument("--node-config", default=DEFAULT_NODE_CONFIG)
    parser.add_argument("--node-bin", default=DEFAULT_NODE_BIN)
    parser.add_argument("--status-file", default=DEFAULT_STATUS_FILE)
    parser.add_argument("--resume-file", default=DEFAULT_RESUME_FILE)
    parser.add_argument("--log-dir", default=DEFAULT_LOG_DIR)
    parser.add_argument("--pid-dir", default=DEFAULT_PID_DIR)
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
        help="Pass --execute to the checkpoint maintainer when starting.",
    )
    return parser.parse_args()


def build_plan_from_args(args: argparse.Namespace) -> dict:
    return build_plan(
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


def main() -> int:
    args = parse_args()
    pid_dir = Path(args.pid_dir)
    if args.status:
        payload = stack_status(pid_dir)
    elif args.stop:
        payload = stop_stack(pid_dir)
    elif args.start:
        payload = start_stack(build_plan_from_args(args), pid_dir=pid_dir)
    else:
        payload = build_plan_from_args(args)
        payload["runner"] = {
            "mode": "dry-run",
            "pid_dir": str(pid_dir),
            "start_requires": "--start",
        }
    print(json.dumps(payload, indent=2, sort_keys=True))
    if payload.get("mode") == "preflight-failed":
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
