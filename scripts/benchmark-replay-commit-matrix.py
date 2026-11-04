#!/usr/bin/env python3
"""MainNet replay batch matrix with RSS, tail latency, and crash-recovery controls."""

from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
from datetime import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import threading
import time

REPO = Path(__file__).resolve().parent.parent
DURATION_SCALE = {"ns": 1e-6, "µs": 1e-3, "ms": 1.0, "s": 1e3}
PROFILE_RE = re.compile(
    r"profile block=(\d+) ([a-z_]+)=([0-9.]+)(ns|µs|ms|s)\b"
)
REPLAY_TOTAL_RE = re.compile(r"profile replay_loop_total=([0-9.]+)(ns|µs|ms|s)\b")
TEST_FILTER = "contiguous_blocks_replay_to_matching_state_roots"
DEFAULT_BLOCK_TEMPLATE = "data/replay-blocks-current/1693127-1694126/{height}.b64"


class ProcessMemoryCounters(ctypes.Structure):
    _fields_ = [
        ("cb", wintypes.DWORD),
        ("PageFaultCount", wintypes.DWORD),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
    ]


def process_memory(handle: int) -> tuple[int, int] | None:
    counters = ProcessMemoryCounters()
    counters.cb = ctypes.sizeof(counters)
    function = ctypes.windll.psapi.GetProcessMemoryInfo
    function.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(ProcessMemoryCounters),
        wintypes.DWORD,
    ]
    function.restype = wintypes.BOOL
    if not function(wintypes.HANDLE(handle), ctypes.byref(counters), counters.cb):
        return None
    return int(counters.WorkingSetSize), int(counters.PeakWorkingSetSize)


class MemoryMonitor:
    def __init__(self, process: subprocess.Popen) -> None:
        self.process = process
        self.stop_event = threading.Event()
        self.max_sampled_rss = 0
        self.peak_rss = 0
        self.samples = 0
        self.thread = threading.Thread(target=self._poll, daemon=True)

    def _sample(self) -> None:
        memory = process_memory(int(self.process._handle))
        if memory is None:
            return
        current, peak = memory
        self.max_sampled_rss = max(self.max_sampled_rss, current)
        self.peak_rss = max(self.peak_rss, peak)
        self.samples += 1

    def _poll(self) -> None:
        while not self.stop_event.is_set():
            self._sample()
            if self.process.poll() is not None:
                break
            self.stop_event.wait(0.02)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, exc_type, exc, traceback) -> None:
        self._sample()
        self.stop_event.set()
        self.thread.join(timeout=2)


def clone_database(source: Path, destination: Path) -> None:
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
        "Isolated benchmark database; never use as production state.\n",
        encoding="utf-8",
    )


def duration_ms(value: str, unit: str) -> float:
    return float(value) * DURATION_SCALE[unit]


def percentile(values: list[float], quantile: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * quantile) - 1)]


def distribution(values: list[float]) -> dict[str, float | int]:
    return {
        "count": len(values),
        "total_ms": sum(values),
        "p50_ms": statistics.median(values),
        "p95_ms": percentile(values, 0.95),
        "p99_ms": percentile(values, 0.99),
        "max_ms": max(values),
    }


def numeric_summary(values: list[float]) -> dict[str, float]:
    return {
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "stdev": statistics.stdev(values) if len(values) > 1 else 0.0,
    }


def execute_replay(
    *,
    binary: Path,
    state_path: Path,
    ledger_path: Path,
    start: int,
    end: int,
    interval: int,
    block_template: str,
    log_stem: str,
    crash_after: int | None = None,
) -> dict:
    stdout_path = REPO / "logs" / f"{log_stem}.stdout.log"
    stderr_path = REPO / "logs" / f"{log_stem}.stderr.log"
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.pop("NEO_EMPTY_REPLAY_COUNT", None)
    env.pop("NEO_REPLAY_TEST_CRASH_AFTER", None)
    env.update(
        {
            "NEO_EMPTY_REPLAY_START": str(start),
            "NEO_EMPTY_REPLAY_END": str(end),
            "NEO_STATE_ROOT_DB_PATH": "data/mainnet-protocol-consistency/StateRoot",
            "NEO_REPLAY_COMMIT_INTERVAL": str(interval),
            "NEO_REPLAY_PROFILE": "1",
            "NEO_REPLAY_BLOCK_PATH": block_template,
            "NEO_REPLAY_STATE_OUTPUT_PATH": str(state_path),
            "NEO_REPLAY_LEDGER_PATH": str(ledger_path),
        }
    )
    if crash_after is not None:
        env["NEO_REPLAY_TEST_CRASH_AFTER"] = str(crash_after)
    command = [str(binary), TEST_FILTER, "--exact", "--ignored", "--nocapture"]
    started = time.perf_counter()
    with stdout_path.open("w", encoding="utf-8", newline="") as stdout, stderr_path.open(
        "w", encoding="utf-8", newline=""
    ) as stderr:
        process = subprocess.Popen(
            command, cwd=REPO, env=env, stdout=stdout, stderr=stderr
        )
        with MemoryMonitor(process) as monitor:
            return_code = process.wait()
    wall_seconds = time.perf_counter() - started
    stdout_text = stdout_path.read_text(encoding="utf-8")
    stderr_text = stderr_path.read_text(encoding="utf-8")
    phases: dict[str, list[float]] = {}
    for match in PROFILE_RE.finditer(stderr_text):
        phases.setdefault(match.group(2), []).append(
            duration_ms(match.group(3), match.group(4))
        )
    total_match = REPLAY_TOTAL_RE.search(stderr_text)
    expected_count = end - start + 1
    completion = (
        f"replay_range={start}..={end} checked={expected_count} reference_gaps=0"
    )
    crash_marker = (
        f"replay_test_crash_after={crash_after} "
        f"last_height={start + crash_after - 1} commit_interval={interval}"
        if crash_after is not None
        else None
    )
    errors = []
    if crash_after is None:
        if return_code != 0:
            errors.append(f"return code {return_code}")
        if "test result: ok. 1 passed;" not in stdout_text:
            errors.append("missing one successful test")
        if completion not in stderr_text:
            errors.append("missing complete state-root verification marker")
        if total_match is None:
            errors.append("missing replay_loop_total")
        if "block_total" not in phases or len(phases["block_total"]) != expected_count:
            errors.append("missing per-block timing samples")
    else:
        if return_code != 70:
            errors.append(f"expected crash return code 70, got {return_code}")
        if crash_marker not in stderr_text:
            errors.append("missing intentional crash marker")
    if monitor.samples == 0 or monitor.peak_rss == 0:
        errors.append("missing process RSS samples")
    return {
        "start": start,
        "end": end,
        "expected_state_roots": expected_count,
        "commit_interval": interval,
        "crash_after": crash_after,
        "return_code": return_code,
        "wall_seconds": wall_seconds,
        "replay_loop_ms": duration_ms(*total_match.groups()) if total_match else None,
        "phase_profiles": {name: distribution(values) for name, values in phases.items()},
        "block_total_ms": phases.get("block_total", []),
        "memory": {
            "samples": monitor.samples,
            "max_sampled_rss_bytes": monitor.max_sampled_rss,
            "peak_rss_bytes": monitor.peak_rss,
        },
        "state_root_verification": crash_after is None and completion in stderr_text,
        "stdout_log": str(stdout_path),
        "stderr_log": str(stderr_path),
        "state_path": str(state_path),
        "ledger_path": str(ledger_path),
        "validation_errors": errors,
    }


def require_valid(run: dict) -> None:
    if run["validation_errors"]:
        raise RuntimeError(
            f"replay validation failed: {run['validation_errors']}; "
            f"logs: {run['stdout_log']}, {run['stderr_log']}"
        )


def common_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--start", type=int, default=1_693_127)
    parser.add_argument("--end", type=int, default=1_694_126)
    parser.add_argument("--block-template", default=DEFAULT_BLOCK_TEMPLATE)
    parser.add_argument("--state-source", type=Path, default=Path("data/replay-state-mainnet"))
    parser.add_argument("--ledger-source", type=Path, default=Path("data/replay-ledger-mainnet"))
    parser.add_argument("--run-id")


def resolved_binary(value: Path) -> Path:
    binary = value if value.is_absolute() else REPO / value
    binary = binary.resolve()
    if not binary.is_file():
        raise ValueError(f"missing benchmark binary: {binary}")
    return binary


def source_path(value: Path) -> Path:
    return (value if value.is_absolute() else REPO / value).resolve()


def output_directory(run_id: str | None, prefix: str) -> Path:
    name = run_id or f"{prefix}-{datetime.now().strftime('%Y%m%d-%H%M%S')}"
    path = REPO / "benchmark-results" / name
    path.mkdir(parents=True, exist_ok=False)
    return path


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def run_matrix(args: argparse.Namespace) -> None:
    intervals = [int(value) for value in args.intervals.split(",")]
    if not intervals or any(value <= 0 for value in intervals):
        raise ValueError("intervals must be comma-separated positive integers")
    binary = resolved_binary(args.binary)
    state_source = source_path(args.state_source)
    ledger_source = source_path(args.ledger_source)
    run_dir = output_directory(args.run_id, "replay-heavy-matrix")
    runs = []
    for sequence, interval in enumerate(intervals, start=1):
        work = run_dir / f"interval-{interval}"
        state_path = work / "state"
        ledger_path = work / "ledger"
        clone_database(state_source, state_path)
        clone_database(ledger_source, ledger_path)
        print(f"[{sequence}/{len(intervals)}] interval={interval}", flush=True)
        run = execute_replay(
            binary=binary,
            state_path=state_path,
            ledger_path=ledger_path,
            start=args.start,
            end=args.end,
            interval=interval,
            block_template=args.block_template,
            log_stem=f"{run_dir.name}-interval-{interval}",
        )
        runs.append(run)
        write_json(
            run_dir / "result.json",
            {
                "kind": "matrix-round",
                "binary": str(binary),
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "state_source": str(state_source),
                "ledger_source": str(ledger_source),
                "block_template": args.block_template,
                "runs": runs,
            },
        )
        require_valid(run)
        print(
            f"    wall={run['wall_seconds']:.6f}s "
            f"loop={run['replay_loop_ms'] / 1000:.6f}s "
            f"p99={run['phase_profiles']['block_total']['p99_ms']:.3f}ms "
            f"peak_rss={run['memory']['peak_rss_bytes'] / 2**20:.1f}MiB PASS",
            flush=True,
        )
    print(f"result={run_dir / 'result.json'}", flush=True)


def read_durable_height(binary: Path, state_path: Path, ledger_path: Path) -> int:
    env = os.environ.copy()
    env.update(
        {
            "NEO_STATE_ROOT_DB_PATH": "data/mainnet-protocol-consistency/StateRoot",
            "NEO_REPLAY_STATE_OUTPUT_PATH": str(state_path),
            "NEO_REPLAY_LEDGER_PATH": str(ledger_path),
        }
    )
    result = subprocess.run(
        [str(binary), "print_replay_state_height", "--exact", "--ignored", "--nocapture"],
        cwd=REPO,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    match = re.search(r"replay_local_root_index=(\d+)", result.stdout)
    if result.returncode != 0 or not match:
        raise RuntimeError(f"failed to read durable height: {result.stdout} {result.stderr}")
    return int(match.group(1))


def run_recovery(args: argparse.Namespace) -> None:
    binary = resolved_binary(args.binary)
    state_source = source_path(args.state_source)
    ledger_source = source_path(args.ledger_source)
    run_dir = output_directory(args.run_id, f"replay-recovery-{args.interval}")
    total_blocks = args.end - args.start + 1
    if not 0 < args.crash_after < total_blocks:
        raise ValueError("crash-after must be inside the requested range")
    committed_blocks = (args.crash_after // args.interval) * args.interval
    durable_height = args.start - 1 + committed_blocks
    replayed_uncommitted = args.crash_after - committed_blocks
    prefill_state = run_dir / "prefill" / "state"
    prefill_ledger = run_dir / "prefill" / "ledger"
    clone_database(state_source, prefill_state)
    clone_database(ledger_source, prefill_ledger)
    print(
        f"prefill interval={args.interval} crash_after={args.crash_after} "
        f"expected_durable={durable_height}",
        flush=True,
    )
    prefill = execute_replay(
        binary=binary,
        state_path=prefill_state,
        ledger_path=prefill_ledger,
        start=args.start,
        end=args.end,
        interval=args.interval,
        block_template=args.block_template,
        log_stem=f"{run_dir.name}-prefill",
        crash_after=args.crash_after,
    )
    require_valid(prefill)
    observed_height = read_durable_height(binary, prefill_state, prefill_ledger)
    if observed_height != durable_height:
        raise RuntimeError(f"durable height {observed_height}, expected {durable_height}")
    resume_start = durable_height + 1
    print(f"resume from {resume_start}", flush=True)
    resume = execute_replay(
        binary=binary,
        state_path=prefill_state,
        ledger_path=prefill_ledger,
        start=resume_start,
        end=args.end,
        interval=args.interval,
        block_template=args.block_template,
        log_stem=f"{run_dir.name}-resume",
    )
    require_valid(resume)
    tail_state = run_dir / "tail-control" / "state"
    tail_ledger = run_dir / "tail-control" / "ledger"
    clone_database(state_source, tail_state)
    clone_database(ledger_source, tail_ledger)
    print(f"tail control from {resume_start}", flush=True)
    tail = execute_replay(
        binary=binary,
        state_path=tail_state,
        ledger_path=tail_ledger,
        start=resume_start,
        end=args.end,
        interval=args.interval,
        block_template=args.block_template,
        log_stem=f"{run_dir.name}-tail-control",
    )
    require_valid(tail)
    result = {
        "kind": "crash-recovery",
        "binary": str(binary),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "commit_interval": args.interval,
        "crash_after": args.crash_after,
        "durable_height": durable_height,
        "observed_durable_height": observed_height,
        "resume_start": resume_start,
        "tail_blocks": args.end - durable_height,
        "replayed_uncommitted_blocks": replayed_uncommitted,
        "prefill": prefill,
        "resume": resume,
        "tail_control": tail,
        "recovery_overhead_seconds": resume["wall_seconds"] - tail["wall_seconds"],
        "recovery_loop_overhead_ms": resume["replay_loop_ms"] - tail["replay_loop_ms"],
    }
    write_json(run_dir / "result.json", result)
    print(
        f"    durable={observed_height} replayed_uncommitted={replayed_uncommitted} "
        f"resume={resume['wall_seconds']:.6f}s tail={tail['wall_seconds']:.6f}s "
        f"overhead={result['recovery_overhead_seconds']:.6f}s",
        flush=True,
    )
    print(f"result={run_dir / 'result.json'}", flush=True)


def summarize(args: argparse.Namespace) -> None:
    documents = [json.loads(path.read_text(encoding="utf-8")) for path in args.results]
    runs = [run for document in documents for run in document["runs"]]
    hashes = {document["binary_sha256"] for document in documents}
    if len(hashes) != 1:
        raise ValueError("all summarized rounds must use the same binary")
    groups = {}
    for interval in sorted({run["commit_interval"] for run in runs}):
        selected = [run for run in runs if run["commit_interval"] == interval]
        if any(run["validation_errors"] for run in selected):
            raise ValueError(f"interval {interval} contains failed runs")
        block_values = [value for run in selected for value in run["block_total_ms"]]
        phase_names = sorted({name for run in selected for name in run["phase_profiles"]})
        groups[str(interval)] = {
            "runs": len(selected),
            "wall_seconds": numeric_summary([run["wall_seconds"] for run in selected]),
            "replay_loop_ms": numeric_summary([run["replay_loop_ms"] for run in selected]),
            "peak_rss_bytes": numeric_summary(
                [run["memory"]["peak_rss_bytes"] for run in selected]
            ),
            "block_total": distribution(block_values),
            "phase_totals_ms": {
                name: numeric_summary(
                    [run["phase_profiles"][name]["total_ms"] for run in selected if name in run["phase_profiles"]]
                )
                for name in phase_names
            },
            "all_state_roots_verified": all(run["state_root_verification"] for run in selected),
        }
    result = {
        "kind": "matrix-summary",
        "binary_sha256": next(iter(hashes)),
        "sources": [str(path) for path in args.results],
        "groups": groups,
    }
    output = args.output or REPO / "benchmark-results" / (
        f"replay-heavy-matrix-summary-{datetime.now().strftime('%Y%m%d-%H%M%S')}.json"
    )
    write_json(output, result)
    print(json.dumps(result, indent=2), flush=True)
    print(f"result={output}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    matrix = subparsers.add_parser("run")
    common_arguments(matrix)
    matrix.add_argument("--intervals", default="1,16,64,256")
    matrix.set_defaults(function=run_matrix)
    recovery = subparsers.add_parser("recover")
    common_arguments(recovery)
    recovery.add_argument("--interval", type=int, required=True)
    recovery.add_argument("--crash-after", type=int, default=700)
    recovery.set_defaults(function=run_recovery)
    summary = subparsers.add_parser("summarize")
    summary.add_argument("results", type=Path, nargs="+")
    summary.add_argument("--output", type=Path)
    summary.set_defaults(function=summarize)
    args = parser.parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
