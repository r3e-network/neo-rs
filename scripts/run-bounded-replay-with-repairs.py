#!/usr/bin/env python3
"""Run bounded replay attempts and repair new GAS drift failures between runs."""

from __future__ import annotations

import argparse
from contextlib import nullcontext
import importlib.util
import json
import sys
from pathlib import Path
from typing import Any, Callable


SCRIPT_DIR = Path(__file__).resolve().parent


def load_script_module(filename: str, module_name: str) -> Any:
    spec = importlib.util.spec_from_file_location(module_name, SCRIPT_DIR / filename)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {filename}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


ReplayRunner = Callable[[], dict]
Repairer = Callable[[int], dict]
LogSizeReader = Callable[[], int]


def run_replay_with_repairs(
    *,
    target_height: int,
    max_attempts: int,
    log_size_reader: LogSizeReader,
    replay_runner: ReplayRunner,
    repairer: Repairer,
) -> dict:
    attempts = []
    last_status = "attempt-limit"
    last_height = None

    for attempt_index in range(1, max_attempts + 1):
        start_offset = log_size_reader()
        report = replay_runner()
        last_status = str(report.get("status", "unknown"))
        last_height = report.get("last_height")
        attempt = {
            "attempt": attempt_index,
            "log_start_offset": start_offset,
            "replay": report,
        }

        if last_status == "target-reached":
            attempt["action"] = "complete"
            attempts.append(attempt)
            return {
                "status": "target-reached",
                "target_height": target_height,
                "last_height": last_height,
                "attempts": attempts,
            }

        if last_status == "timeout":
            try:
                repair = repairer(start_offset)
            except Exception as exc:  # pylint: disable=broad-except
                attempt["action"] = "retry-after-timeout"
                attempt["repair_error"] = str(exc)
            else:
                attempt["action"] = "repair-and-retry"
                attempt["repair"] = repair
            attempts.append(attempt)
            continue

        try:
            repair = repairer(start_offset)
        except Exception as exc:  # pylint: disable=broad-except
            attempt["action"] = "stop-unrepaired-exit"
            attempt["repair_error"] = str(exc)
            attempts.append(attempt)
            return {
                "status": last_status,
                "target_height": target_height,
                "last_height": last_height,
                "attempts": attempts,
            }

        attempt["action"] = "repair-and-retry"
        attempt["repair"] = repair
        attempts.append(attempt)

    return {
        "status": "attempt-limit",
        "last_replay_status": last_status,
        "target_height": target_height,
        "last_height": last_height,
        "attempts": attempts,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run bounded MainNet replay repeatedly. Timeouts continue from the "
            "same DB, while process exits trigger a bounded GAS repair using only "
            "the log bytes produced by that attempt."
        )
    )
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--db", required=True, type=Path)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--target-height", required=True, type=int)
    parser.add_argument("--node-bin", default=Path("target/release/neo-node"), type=Path)
    parser.add_argument("--probe-bin", default=Path("target/release/neo-db-probe"), type=Path)
    parser.add_argument("--rpc", default="http://127.0.0.1:21332")
    parser.add_argument("--reference-rpc", default="http://seed1.neo.org:10332")
    parser.add_argument("--poll-interval", default=30.0, type=float)
    parser.add_argument("--max-seconds", default=900.0, type=float)
    parser.add_argument("--max-attempts", default=20, type=int)
    parser.add_argument(
        "--node-output-log",
        default=None,
        type=Path,
        help="Append neo-node stdout/stderr to this file for every replay attempt.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    runner = load_script_module("run-bounded-mainnet-replay.py", "run_bounded_mainnet_replay")
    repair = load_script_module("repair-bounded-replay-gas.py", "repair_bounded_replay_gas")
    current_log_start_offset = {"value": 0}

    def log_size() -> int:
        offset = args.log.stat().st_size if args.log.exists() else 0
        current_log_start_offset["value"] = offset
        return offset

    def repairable_failure_seen() -> bool:
        if not args.log.exists():
            return False
        text = repair.read_log_text(args.log, current_log_start_offset["value"])
        return bool(repair.parse_gas_burn_failures(text))

    def replay_once() -> dict:
        return runner.run_until_target(
            command=runner.node_command(args.node_bin, args.config, args.target_height),
            rpc_url=args.rpc,
            target_height=args.target_height,
            poll_interval=args.poll_interval,
            max_seconds=args.max_seconds,
            repairable_failure_detector=repairable_failure_seen,
            height_reader=lambda: runner.read_probe_ledger_height(
                args.db,
                args.probe_bin,
            ),
            node_output=node_output_handle,
        )

    def repair_once(log_start_offset: int) -> dict:
        return repair.repair_bounded_replay_gas(
            db_path=args.db,
            log_path=args.log,
            probe_bin=args.probe_bin,
            reference_rpc=args.reference_rpc,
            log_start_offset=log_start_offset,
            apply=True,
        )

    if args.node_output_log is not None:
        args.node_output_log.parent.mkdir(parents=True, exist_ok=True)
    with (
        args.node_output_log.open("a", encoding="utf-8")
        if args.node_output_log is not None
        else nullcontext(None)
    ) as node_output_handle:
        summary = run_replay_with_repairs(
            target_height=args.target_height,
            max_attempts=args.max_attempts,
            log_size_reader=log_size,
            replay_runner=replay_once,
            repairer=repair_once,
        )
    print(json.dumps(summary, indent=2, sort_keys=True))
    if summary["status"] == "target-reached":
        return 0
    if summary["status"] in {"timeout", "attempt-limit"}:
        return 124
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
