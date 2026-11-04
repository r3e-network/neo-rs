#!/usr/bin/env python3
"""Run fail-closed, resumable contiguous execution over a Neo ACC range."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid
from datetime import datetime, timezone

REPO = Path(__file__).resolve().parents[1]
STATUS_SCHEMA = 2


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def write_status(status_path: Path, status: dict) -> None:
    temporary_path = status_path.with_suffix(status_path.suffix + ".tmp")
    temporary_path.write_text(json.dumps(status, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary_path, status_path)


def run_logged(command: list[str], log_path: Path, env: dict[str, str]) -> None:
    with log_path.open("w", encoding="utf-8") as log:
        result = subprocess.run(command, cwd=REPO, env=env, stdout=log, stderr=log)
    if result.returncode != 0:
        raise RuntimeError(f"command failed ({result.returncode}); see {log_path}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--start", type=int, required=True)
    parser.add_argument("--end", type=int, required=True)
    parser.add_argument("--step", type=int, default=100_000)
    parser.add_argument("--commit-interval", type=int, default=64)
    parser.add_argument("--max-seconds", type=float)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--acc", default="data/bootstrap/chain.0.acc")
    parser.add_argument("--state-output", default="data/replay-state-mainnet")
    parser.add_argument("--ledger-output", default="data/replay-ledger-mainnet")
    parser.add_argument(
        "--test-binary",
        default=(
            "target/replay-overlay/debug/deps/"
            "mainnet_contiguous_replay-5b54d4f2dcae8741.exe"
        ),
    )
    args = parser.parse_args()
    if args.start < 1 or args.end < args.start or args.step < 1:
        parser.error("require start >= 1, end >= start, and step >= 1")
    if args.commit_interval < 1:
        parser.error("require commit-interval >= 1")
    if args.max_seconds is not None and args.max_seconds <= 0:
        parser.error("require max-seconds > 0")

    binary = (REPO / args.test_binary).resolve()
    state_output = (REPO / args.state_output).resolve()
    ledger_output = (REPO / args.ledger_output).resolve()
    if not binary.is_file():
        raise FileNotFoundError(binary)
    if state_output == ledger_output:
        parser.error("state-output and ledger-output must be different")
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    status_path = state_output.parent / f"{state_output.name}-status.json"
    checkpoint_path = state_output.parent / f"{state_output.name}-checkpoints.jsonl"
    (REPO / "logs").mkdir(parents=True, exist_ok=True)

    expected_configuration = {
        "schema": STATUS_SCHEMA,
        "requested_start": args.start,
        "requested_end": args.end,
        "binary": str(binary),
        "binary_sha256": binary_hash,
        "state_output": str(state_output),
        "ledger_output": str(ledger_output),
        "commit_interval": args.commit_interval,
    }
    if args.resume:
        if not status_path.is_file():
            raise FileNotFoundError(f"cannot resume without status file: {status_path}")
        status = json.loads(status_path.read_text(encoding="utf-8"))
        mismatches = [
            key
            for key, expected in expected_configuration.items()
            if status.get(key) != expected
        ]
        if mismatches:
            raise RuntimeError(
                "resume configuration mismatch: " + ", ".join(sorted(mismatches))
            )
        if status.get("status") not in {"clean", "completed"}:
            raise RuntimeError(f"cannot resume status {status.get('status')!r}")
        start = status["completed_end"] + 1
        if start > args.end:
            print(
                f"already completed {args.start}..={args.end}; "
                f"active_wall_seconds={status['active_wall_seconds']:.3f}",
                flush=True,
            )
            return
        run_id = status["run_id"]
        print(
            f"resuming run={run_id} at {start}; "
            f"active_wall_seconds={status['active_wall_seconds']:.3f}",
            flush=True,
        )
    else:
        start = args.start
        if status_path.exists() or state_output.exists() or ledger_output.exists():
            raise FileExistsError(
                "fresh replay requires new status, state, and ledger paths; "
                "use --resume to continue an existing run"
            )
        run_id = uuid.uuid4().hex
        status = {
            **expected_configuration,
            "status": "clean",
            "run_id": run_id,
            "started_at": utc_now(),
            "active_wall_seconds": 0.0,
            "completed_start": args.start,
            "completed_end": args.start - 1,
            "reference_gaps": 0,
        }
        write_status(status_path, status)
        print(f"starting run={run_id} from {args.start}", flush=True)

    invocation_started = time.perf_counter()
    while start <= args.end:
        segment_started = time.perf_counter()
        end = min(start + args.step - 1, args.end)
        block_dir = (
            REPO / "data" / "replay-blocks-current" / run_id / f"{start}-{end}"
        )
        log_prefix = REPO / "logs" / f"audit-current-{run_id}-{start}-{end}"
        extract_log = log_prefix.with_name(log_prefix.name + "-extract.log")
        out_log = log_prefix.with_name(log_prefix.name + "-replay.out.log")
        err_log = log_prefix.with_name(log_prefix.name + "-replay.err.log")
        print(f"extracting {start}..={end}", flush=True)
        extract_started = time.perf_counter()
        run_logged(
            [
                sys.executable,
                str(REPO / "scripts" / "extract-acc-range.py"),
                str(REPO / args.acc),
                str(block_dir),
                str(max(0, start - 2)),
                str(end),
            ],
            extract_log,
            os.environ.copy(),
        )
        extract_seconds = time.perf_counter() - extract_started

        env = os.environ.copy()
        env.pop("NEO_REPLAY_TEST_CRASH_AFTER", None)
        env.pop("NEO_EMPTY_REPLAY_COUNT", None)
        env.update(
            {
                "NEO_EMPTY_REPLAY_START": str(start),
                "NEO_EMPTY_REPLAY_END": str(end),
                "NEO_REPLAY_BLOCK_PATH": str(block_dir / "{height}.b64"),
                "NEO_REPLAY_STATE_OUTPUT_PATH": str(state_output),
                "NEO_REPLAY_LEDGER_PATH": str(ledger_output),
                "NEO_REPLAY_COMMIT_INTERVAL": str(args.commit_interval),
                "NEO_REPLAY_PROFILE": "1",
            }
        )
        # Every block is executed and root-checked. Only durable I/O is grouped;
        # after an interruption, resume from durable_height + 1 and re-verify.
        print(f"replaying {start}..={end}", flush=True)
        replay_started = time.perf_counter()
        with out_log.open("w", encoding="utf-8") as stdout, err_log.open(
            "w", encoding="utf-8"
        ) as stderr:
            result = subprocess.run(
                [
                    str(binary),
                    "contiguous_blocks_replay_to_matching_state_roots",
                    "--ignored",
                    "--nocapture",
                ],
                cwd=REPO,
                env=env,
                stdout=stdout,
                stderr=stderr,
            )
        replay_seconds = time.perf_counter() - replay_started
        if result.returncode != 0:
            raise RuntimeError(
                f"replay failed at range {start}..={end}; see {out_log} and {err_log}"
            )

        expected_count = end - start + 1
        expected_marker = (
            f"replay_range={start}..={end} checked={expected_count} reference_gaps=0"
        )
        stderr_text = err_log.read_text(encoding="utf-8")
        stdout_text = out_log.read_text(encoding="utf-8")
        if expected_marker not in stderr_text or (
            "test result: ok. 1 passed;" not in stdout_text
        ):
            raise RuntimeError(f"missing complete replay evidence for {start}..={end}")

        shutil.rmtree(block_dir)
        segment_seconds = time.perf_counter() - segment_started
        status.update(
            {
                "status": "completed" if end == args.end else "clean",
                "completed_end": end,
                "next_start": end + 1,
                "last_range": [start, end],
                "last_range_checked": expected_count,
                "reference_gaps": 0,
                "last_segment_wall_seconds": segment_seconds,
                "last_extract_seconds": extract_seconds,
                "last_replay_seconds": replay_seconds,
                "active_wall_seconds": status["active_wall_seconds"] + segment_seconds,
                "calendar_elapsed_seconds": (
                    datetime.now(timezone.utc)
                    - datetime.fromisoformat(status["started_at"])
                ).total_seconds(),
                "updated_at": utc_now(),
            }
        )
        write_status(status_path, status)
        checkpoint = {
            key: status[key]
            for key in (
                "schema",
                "run_id",
                "completed_end",
                "next_start",
                "last_range",
                "last_range_checked",
                "reference_gaps",
                "last_segment_wall_seconds",
                "last_extract_seconds",
                "last_replay_seconds",
                "active_wall_seconds",
                "updated_at",
            )
        }
        with checkpoint_path.open("a", encoding="utf-8") as checkpoints:
            checkpoints.write(json.dumps(checkpoint) + "\n")
            checkpoints.flush()
            os.fsync(checkpoints.fileno())
        print(
            f"matched {start}..={end}; checked={expected_count}; "
            f"segment={segment_seconds:.3f}s; "
            f"active={status['active_wall_seconds']:.3f}s",
            flush=True,
        )
        start = end + 1
        if args.max_seconds is not None and (
            time.perf_counter() - invocation_started >= args.max_seconds
        ):
            # Stop only after a successful, durable segment checkpoint.
            print("max-seconds reached after durable checkpoint", flush=True)
            break


if __name__ == "__main__":
    main()
