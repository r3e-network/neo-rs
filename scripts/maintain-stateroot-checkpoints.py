#!/usr/bin/env python3
"""Maintain checkpoint phases reported by continuous state-root validation."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Any, Callable


DEFAULT_STATUS_FILE = "/tmp/stateroot-validation.json"
DEFAULT_DATA_DIR = "./data"
DEFAULT_CHECKPOINT_SCRIPT = "scripts/checkpoint-on-height.sh"
DEFAULT_WAITING_INTERVAL_SECONDS = 30


def checkpoint_exists(path: Path) -> bool:
    return (
        path.is_dir()
        and (path / "mainnet").is_dir()
        and (path / "StateRoot").is_dir()
        and (path / "CHECKPOINT_INFO").is_file()
        and not (path / "CHECKPOINT_IN_PROGRESS").exists()
    )


def checkpoint_is_blocked(path: Path) -> bool:
    return path.exists() and not checkpoint_exists(path)


def checkpoint_height_is_current(status: dict, height: int) -> bool:
    local_block_count = status.get("local_block_count")
    local_state_height = status.get("local_state_height")
    if local_block_count is None or local_state_height is None:
        return True

    if int(local_block_count) != height + 1:
        return False
    if int(local_state_height) != height:
        return False

    local_validated_height = status.get("local_validated_height")
    if local_validated_height is not None and int(local_validated_height) != height:
        return False

    last_validated_block = status.get("last_validated_block")
    if last_validated_block is not None and int(last_validated_block) < height:
        return False

    return True


def load_status(path: Path) -> dict:
    if not path.exists():
        return {
            "status": f"missing status file: {path}",
            "checkpoint_stages": [],
        }
    return json.loads(path.read_text(encoding="utf-8"))


def format_network_magic(value: Any) -> str:
    if isinstance(value, int):
        return f"{value:08X}"
    if isinstance(value, str):
        stripped = value.strip()
        return f"{int(stripped, 0):08X}"
    raise ValueError(f"unsupported network_magic value: {value!r}")


def derive_checkpoint_paths_from_config(config_path: Path) -> dict[str, Path]:
    with config_path.open("rb") as handle:
        config = tomllib.load(handle)

    storage = config.get("storage") or {}
    state_service = config.get("state_service") or {}
    network = config.get("network") or {}

    chain_db = Path(storage.get("data_dir") or storage.get("path") or DEFAULT_DATA_DIR)
    data_dir = chain_db.parent if chain_db.name else Path(DEFAULT_DATA_DIR)

    magic = format_network_magic(network.get("network_magic", 0))
    state_template = str(state_service.get("path") or f"{DEFAULT_DATA_DIR}/Plugins/mainnet/StateRoot")
    stateroot_db = Path(state_template.replace("{0}", magic))

    return {
        "data_dir": data_dir,
        "chain_db": chain_db,
        "stateroot_db": stateroot_db,
    }


def build_checkpoint_plan(
    status: dict,
    *,
    checkpoint_root: Path,
    data_dir: Path,
    writer_pid: str,
    script_path: Path,
    chain_db: Path | None,
    stateroot_db: Path | None,
) -> dict:
    plan_context = {
        "checkpoint_root": str(checkpoint_root),
        "data_dir": str(data_dir),
        "chain_db": str(chain_db) if chain_db is not None else None,
        "stateroot_db": str(stateroot_db) if stateroot_db is not None else None,
    }
    stages = status.get("checkpoint_stages") or []
    if not stages:
        return {
            "status": "waiting",
            "actions": [],
            **plan_context,
        }

    actions = []
    blocked = False
    deferred = False
    planned_creates: set[Path] = set()
    for stage in stages:
        height = int(stage["height"])
        stage_name = str(stage["stage"])
        checkpoint_path = checkpoint_root / f"h{height}"
        item = {
            "stage": stage_name,
            "height": height,
            "label": stage.get("label", f"{stage_name}-h{height}"),
            "checkpoint_path": str(checkpoint_path),
        }
        if checkpoint_exists(checkpoint_path):
            item["action"] = "skip"
        elif checkpoint_is_blocked(checkpoint_path):
            item["action"] = "blocked"
            item["reason"] = "incomplete checkpoint directory exists"
            blocked = True
        elif checkpoint_path in planned_creates:
            item["action"] = "skip"
            item["reason"] = "checkpoint height already planned"
        elif not checkpoint_height_is_current(status, height):
            item["action"] = "defer"
            item["reason"] = (
                "checkpoint height is not the current durable chain height; "
                "historical snapshots cannot be created from a later live DB"
            )
            deferred = True
        else:
            item["action"] = "create"
            planned_creates.add(checkpoint_path)
            command = [
                str(script_path),
                writer_pid,
                "--once",
                "--height",
                str(height),
                "--data-dir",
                str(data_dir),
                "--root",
                str(checkpoint_root),
            ]
            if chain_db is not None:
                command.extend(["--chain-db", str(chain_db)])
            if stateroot_db is not None:
                command.extend(["--stateroot-db", str(stateroot_db)])
            item["command"] = command
        actions.append(item)

    return {
        "status": "blocked" if blocked else "waiting" if deferred else "ready",
        "actions": actions,
        **plan_context,
    }


def execute_plan(
    plan: dict,
    *,
    execute: bool,
    runner: Callable[[list[str]], Any] | None = None,
) -> dict:
    executed = 0
    skipped = 0
    runner = runner or run_command
    for action in plan.get("actions", []):
        if action.get("action") == "skip":
            skipped += 1
            continue
        if action.get("action") != "create":
            continue
        if execute:
            runner(action["command"])
            executed += 1
    result = dict(plan)
    result["executed"] = executed
    result["skipped"] = skipped
    result["dry_run"] = not execute
    return result


def run_watch_loop(
    *,
    status_loader: Callable[[], dict],
    plan_builder: Callable[[dict], dict],
    execute: bool,
    runner: Callable[[list[str]], Any] | None,
    interval_seconds: int,
    waiting_interval_seconds: int | None = None,
    max_iterations: int | None = None,
    sleep_fn: Callable[[int], Any] = time.sleep,
    result_writer: Callable[[dict], Any] | None = None,
) -> dict:
    iterations = 0
    total_executed = 0
    total_skipped = 0

    while True:
        status = status_loader()
        plan = plan_builder(status)
        result = execute_plan(plan, execute=execute, runner=runner)
        iterations += 1
        total_executed += result.get("executed", 0)
        total_skipped += result.get("skipped", 0)
        if result_writer is not None:
            result_writer(result)

        if max_iterations is not None and iterations >= max_iterations:
            break
        sleep_seconds = interval_seconds
        if result.get("status") == "waiting" and waiting_interval_seconds is not None:
            sleep_seconds = min(interval_seconds, waiting_interval_seconds)
        sleep_fn(sleep_seconds)

    return {
        "mode": "watch",
        "iterations": iterations,
        "executed": total_executed,
        "skipped": total_skipped,
        "dry_run": not execute,
    }


def run_command(command: list[str]) -> None:
    subprocess.run(command, check=True)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Maintain base/mid/latest checkpoints from a state-root status file."
    )
    parser.add_argument(
        "--status-file",
        default=DEFAULT_STATUS_FILE,
        help=f"Continuous validator status JSON (default: {DEFAULT_STATUS_FILE})",
    )
    parser.add_argument(
        "--node-config",
        default=None,
        help="Optional neo-node TOML used to infer --data-dir, --chain-db, and --stateroot-db.",
    )
    parser.add_argument(
        "--data-dir",
        default=None,
        help=f"neo-rs data root passed to checkpoint-on-height (default: {DEFAULT_DATA_DIR})",
    )
    parser.add_argument(
        "--chain-db",
        default=None,
        help="Explicit chain RocksDB path for validation/replay layouts.",
    )
    parser.add_argument(
        "--stateroot-db",
        default=None,
        help="Explicit StateRoot RocksDB path for validation/replay layouts.",
    )
    parser.add_argument(
        "--root",
        default=None,
        help="Checkpoint root (default: <data-dir>/checkpoints)",
    )
    parser.add_argument(
        "--writer-pid",
        default="none",
        help="neo-node writer PID, or 'none' for stopped/offline data dirs",
    )
    parser.add_argument(
        "--script",
        default=DEFAULT_CHECKPOINT_SCRIPT,
        help=f"Checkpoint script path (default: {DEFAULT_CHECKPOINT_SCRIPT})",
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="Actually create missing checkpoints. Default is JSON dry-run only.",
    )
    parser.add_argument(
        "--watch-interval",
        type=int,
        default=None,
        help="Repeat maintenance every N seconds. Default runs once.",
    )
    parser.add_argument(
        "--waiting-interval",
        type=int,
        default=DEFAULT_WAITING_INTERVAL_SECONDS,
        help=(
            "Retry interval while the validator status has no checkpoint stages "
            f"(default: {DEFAULT_WAITING_INTERVAL_SECONDS}s)."
        ),
    )
    parser.add_argument(
        "--max-iterations",
        type=int,
        default=None,
        help=argparse.SUPPRESS,
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    inferred_paths = (
        derive_checkpoint_paths_from_config(Path(args.node_config))
        if args.node_config
        else {}
    )
    data_dir = Path(args.data_dir) if args.data_dir else inferred_paths.get("data_dir", Path(DEFAULT_DATA_DIR))
    checkpoint_root = Path(args.root) if args.root else data_dir / "checkpoints"
    chain_db = Path(args.chain_db) if args.chain_db else inferred_paths.get("chain_db")
    stateroot_db = (
        Path(args.stateroot_db)
        if args.stateroot_db
        else inferred_paths.get("stateroot_db")
    )

    def status_loader() -> dict:
        return load_status(Path(args.status_file))

    def plan_builder(status: dict) -> dict:
        return build_checkpoint_plan(
            status,
            checkpoint_root=checkpoint_root,
            data_dir=data_dir,
            writer_pid=args.writer_pid,
            script_path=Path(args.script),
            chain_db=chain_db,
            stateroot_db=stateroot_db,
        )

    try:
        if args.watch_interval is None:
            result = execute_plan(plan_builder(status_loader()), execute=args.execute)
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            if args.watch_interval < 1:
                raise ValueError("--watch-interval must be >= 1")
            if args.waiting_interval < 1:
                raise ValueError("--waiting-interval must be >= 1")

            def write_result(result: dict) -> None:
                print(json.dumps(result, sort_keys=True), flush=True)

            summary = run_watch_loop(
                status_loader=status_loader,
                plan_builder=plan_builder,
                execute=args.execute,
                runner=None,
                interval_seconds=args.watch_interval,
                waiting_interval_seconds=args.waiting_interval,
                max_iterations=args.max_iterations,
                result_writer=write_result,
            )
            print(json.dumps(summary, indent=2, sort_keys=True))
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
