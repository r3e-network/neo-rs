#!/usr/bin/env python3
"""Interleaved same-binary A/B benchmark for the contiguous MainNet replay."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time
from datetime import datetime

DURATION_SCALE = {"ns": 1e-6, "µs": 1e-3, "ms": 1.0, "s": 1e3}
PROFILE_RE = re.compile(
    r"profile block=(\d+) ([a-z_]+)=([0-9.]+)(ns|µs|ms|s)\b"
)
REPLAY_TOTAL_RE = re.compile(
    r"profile replay_loop_total=([0-9.]+)(ns|µs|ms|s)\b"
)


def duration_ms(value: str, unit: str) -> float:
    return float(value) * DURATION_SCALE[unit]


def summarize(values: list[float]) -> dict[str, float]:
    return {
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "stdev": statistics.stdev(values) if len(values) > 1 else 0.0,
    }


def clone_database(source: Path, destination: Path) -> None:
    """Clone a RocksDB checkpoint using hard links for immutable table files."""
    import shutil

    if not source.is_dir():
        raise ValueError(f"missing clone source: {source}")
    if destination.exists():
        raise ValueError(f"clone destination already exists: {destination}")
    destination.mkdir(parents=True)
    for entry in source.iterdir():
        if not entry.is_file() or entry.name == "LOCK" or entry.name.startswith("LOG"):
            continue
        target = destination / entry.name
        if entry.suffix.lower() in {".sst", ".ldb"}:
            target.hardlink_to(entry)
        else:
            shutil.copy2(entry, target)
    (destination / "BENCHMARK_ONLY").write_text(
        "Isolated benchmark overlay; never use as production state.\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--start", type=int, default=13_139_251)
    parser.add_argument("--end", type=int, default=13_141_250)
    parser.add_argument("--rounds", type=int, default=3, choices=(1, 2, 3))
    parser.add_argument(
        "--block-template",
        default="data/replay-blocks-current/13139251-13141250/{height}.b64",
    )
    parser.add_argument("--state-source", type=Path, default=Path("data/replay-state-mainnet"))
    parser.add_argument("--ledger-source", type=Path, default=Path("data/replay-ledger-mainnet"))
    parser.add_argument(
        "--state-db", default="data/mainnet-protocol-consistency/StateRoot"
    )
    parser.add_argument("--output-root", type=Path, default=Path("benchmark-results"))
    args = parser.parse_args()
    if args.start < 1 or args.end < args.start:
        parser.error("require 1 <= start <= end")

    repo = Path(__file__).resolve().parent.parent
    binary = (repo / args.binary).resolve() if not args.binary.is_absolute() else args.binary
    state_source = (repo / args.state_source).resolve()
    ledger_source = (repo / args.ledger_source).resolve()
    if not binary.is_file():
        parser.error(f"benchmark binary does not exist: {binary}")
    expected_count = args.end - args.start + 1
    run_id = datetime.now().strftime("%Y%m%d-%H%M%S")
    output_root = args.output_root
    if not output_root.is_absolute():
        output_root = repo / output_root
    run_dir = output_root / f"replay-commit-ab-{run_id}"
    logs_dir = repo / "logs"
    run_dir.mkdir(parents=True, exist_ok=False)
    logs_dir.mkdir(parents=True, exist_ok=True)
    binary_sha256 = hashlib.sha256(binary.read_bytes()).hexdigest()
    order = [interval for pair in ((1, 64), (64, 1), (1, 64)) for interval in pair]
    order = order[: args.rounds * 2]
    runs: list[dict] = []

    for sequence, interval in enumerate(order, start=1):
        round_number = (sequence + 1) // 2
        state_path = run_dir / f"run-{sequence}-interval-{interval}" / "state"
        ledger_path = run_dir / f"run-{sequence}-interval-{interval}" / "ledger"
        clone_database(state_source, state_path)
        clone_database(ledger_source, ledger_path)
        stdout_path = logs_dir / f"{run_dir.name}-run-{sequence}-interval-{interval}.stdout.log"
        stderr_path = logs_dir / f"{run_dir.name}-run-{sequence}-interval-{interval}.stderr.log"
        env = os.environ.copy()
        env.pop("NEO_EMPTY_REPLAY_COUNT", None)
        env.update(
            {
                "NEO_EMPTY_REPLAY_START": str(args.start),
                "NEO_EMPTY_REPLAY_END": str(args.end),
                "NEO_STATE_ROOT_DB_PATH": args.state_db,
                "NEO_REPLAY_COMMIT_INTERVAL": str(interval),
                "NEO_REPLAY_PROFILE": "1",
                "NEO_REPLAY_BLOCK_PATH": args.block_template,
                "NEO_REPLAY_STATE_OUTPUT_PATH": str(state_path),
                "NEO_REPLAY_LEDGER_PATH": str(ledger_path),
            }
        )
        command = [
            str(binary),
            "contiguous_blocks_replay_to_matching_state_roots",
            "--exact",
            "--ignored",
            "--nocapture",
        ]
        print(
            f"[{sequence}/{len(order)}] round={round_number} interval={interval} "
            f"range={args.start}..={args.end}",
            flush=True,
        )
        started = time.perf_counter()
        with stdout_path.open("w", encoding="utf-8", newline="") as stdout, stderr_path.open(
            "w", encoding="utf-8", newline=""
        ) as stderr:
            completed = subprocess.run(
                command,
                cwd=repo,
                env=env,
                stdout=stdout,
                stderr=stderr,
                check=False,
            )
        wall_seconds = time.perf_counter() - started
        stdout_text = stdout_path.read_text(encoding="utf-8")
        stderr_text = stderr_path.read_text(encoding="utf-8")
        expected_completion = (
            f"replay_range={args.start}..={args.end} "
            f"checked={expected_count} reference_gaps=0"
        )
        validation_errors = []
        if completed.returncode != 0:
            validation_errors.append(f"return code {completed.returncode}")
        if "test result: ok." not in stdout_text:
            validation_errors.append("missing successful test result")
        if expected_completion not in stderr_text:
            validation_errors.append("missing complete state-root verification marker")
        total_match = REPLAY_TOTAL_RE.search(stderr_text)
        if not total_match:
            validation_errors.append("missing replay_loop_total profile")
        phases: dict[str, float] = {}
        for match in PROFILE_RE.finditer(stderr_text):
            name = match.group(2)
            phases[name] = phases.get(name, 0.0) + duration_ms(
                match.group(3), match.group(4)
            )
        run = {
            "sequence": sequence,
            "round": round_number,
            "commit_interval": interval,
            "command": command,
            "state_source": str(state_source),
            "ledger_source": str(ledger_source),
            "environment": {
                key: env[key]
                for key in (
                    "NEO_EMPTY_REPLAY_START",
                    "NEO_EMPTY_REPLAY_END",
                    "NEO_STATE_ROOT_DB_PATH",
                    "NEO_REPLAY_COMMIT_INTERVAL",
                    "NEO_REPLAY_BLOCK_PATH",
                    "NEO_REPLAY_STATE_OUTPUT_PATH",
                    "NEO_REPLAY_LEDGER_PATH",
                )
            },
            "return_code": completed.returncode,
            "wall_seconds": wall_seconds,
            "replay_loop_ms": duration_ms(*total_match.groups())
            if total_match
            else None,
            "phase_totals_ms": phases,
            "state_root_verification": expected_completion in stderr_text,
            "stdout_log": str(stdout_path),
            "stderr_log": str(stderr_path),
            "validation_errors": validation_errors,
        }
        runs.append(run)
        result_path = run_dir / "result.json"
        result_path.write_text(
            json.dumps(
                {
                    "run_id": run_dir.name,
                    "binary": str(binary),
                    "binary_sha256": binary_sha256,
                    "start": args.start,
                    "end": args.end,
                    "expected_state_roots": expected_count,
                    "order": order,
                    "runs": runs,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        print(
            f"    wall={wall_seconds:.6f}s "
            f"replay_loop={(run['replay_loop_ms'] or 0.0) / 1000:.6f}s "
            f"validation={'PASS' if not validation_errors else 'FAIL'}",
            flush=True,
        )
        if validation_errors:
            raise RuntimeError(
                f"benchmark run {sequence} failed validation: {validation_errors}; "
                f"logs: {stdout_path}, {stderr_path}"
            )

    groups = {}
    for interval in (1, 64):
        selected = [run for run in runs if run["commit_interval"] == interval]
        phase_names = sorted({name for run in selected for name in run["phase_totals_ms"]})
        groups[str(interval)] = {
            "wall_seconds": summarize([run["wall_seconds"] for run in selected]),
            "replay_loop_ms": summarize([run["replay_loop_ms"] for run in selected]),
            "phase_totals_ms": {
                name: summarize([run["phase_totals_ms"].get(name, 0.0) for run in selected])
                for name in phase_names
            },
            "all_state_roots_verified": all(
                run["state_root_verification"] and not run["validation_errors"]
                for run in selected
            ),
        }
    baseline_loop = groups["1"]["replay_loop_ms"]["median"]
    batched_loop = groups["64"]["replay_loop_ms"]["median"]
    baseline_wall = groups["1"]["wall_seconds"]["median"]
    batched_wall = groups["64"]["wall_seconds"]["median"]
    summary = {
        "groups": groups,
        "median_replay_loop_reduction_percent": (baseline_loop - batched_loop)
        / baseline_loop
        * 100,
        "median_replay_loop_speedup": baseline_loop / batched_loop,
        "median_wall_reduction_percent": (baseline_wall - batched_wall)
        / baseline_wall
        * 100,
        "median_wall_speedup": baseline_wall / batched_wall,
    }
    result = {
        "run_id": run_dir.name,
        "binary": str(binary),
        "binary_sha256": binary_sha256,
        "start": args.start,
        "end": args.end,
        "expected_state_roots": expected_count,
        "order": order,
        "runs": runs,
        "summary": summary,
    }
    result_path = run_dir / "result.json"
    result_path.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2), flush=True)
    print(f"result={result_path}", flush=True)


if __name__ == "__main__":
    main()
