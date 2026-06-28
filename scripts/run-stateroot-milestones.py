#!/usr/bin/env python3
"""Run StateRoot validation milestones and checkpoint each successful height."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable


DEFAULT_NODE_BIN = "target/debug/neo-node"
DEFAULT_PROBE_BIN = "target/debug/neo-db-probe"
DEFAULT_RPC = "http://127.0.0.1:21332"
DEFAULT_REFERENCE_RPCS = [
    "http://seed1.neo.org:10332",
    "http://seed2.neo.org:10332",
    "http://seed3.neo.org:10332",
    "http://seed4.neo.org:10332",
    "http://seed5.neo.org:10332",
]


def parse_height_values(values: list[str]) -> list[int]:
    heights: list[int] = []
    for value in values:
        for part in str(value).split(","):
            part = part.strip()
            if not part:
                continue
            try:
                height = int(part, 10)
            except ValueError as exc:
                raise ValueError(f"invalid milestone height: {part}") from exc
            if height < 0:
                raise ValueError(f"milestone height must be >= 0: {height}")
            heights.append(height)
    if not heights:
        raise ValueError("at least one --milestone height is required")
    if heights != sorted(set(heights)):
        raise ValueError("milestone heights must be unique and strictly increasing")
    return heights


def parse_last_json_object(text: str) -> dict:
    decoder = json.JSONDecoder()
    last: dict | None = None
    for index, char in enumerate(text):
        if char != "{":
            continue
        try:
            candidate, end = decoder.raw_decode(text[index:])
        except json.JSONDecodeError:
            continue
        if isinstance(candidate, dict) and not text[index + end :].strip():
            last = candidate
            break
        if isinstance(candidate, dict):
            last = candidate
    if last is None:
        raise ValueError("command output did not contain a JSON object")
    return last


def normalize_reference_urls(values: list[str] | None) -> list[str]:
    urls: list[str] = []
    for value in values or []:
        for part in str(value).split(","):
            part = part.strip()
            if part:
                urls.append(part)
    return urls


def bounded_command(
    *,
    config: Path,
    node_bin: Path,
    rpc_url: str,
    target_height: int,
    poll_interval: float,
    max_seconds: float,
    chain_db: Path,
    stateroot_db: Path,
    probe_bin: Path,
    references: list[str],
    node_output_log: Path,
    metrics_url: str | None = None,
) -> list[str]:
    command = [
        "python3",
        "scripts/run-bounded-mainnet-replay.py",
        "--config",
        str(config),
        "--node-bin",
        str(node_bin),
        "--rpc",
        rpc_url,
        "--target-height",
        str(target_height),
        "--poll-interval",
        str(poll_interval),
        "--max-seconds",
        str(max_seconds),
        "--db",
        str(chain_db),
        "--stateroot-db",
        str(stateroot_db),
        "--probe-bin",
        str(probe_bin),
        "--require-stateroot-height-match",
    ]
    if references:
        command.extend(
            [
                "--reference",
                ",".join(references),
                "--require-reference-stateroot-match",
            ]
        )
    if metrics_url:
        command.extend(["--metrics-url", metrics_url])
    command.extend(["--node-output-log", str(node_output_log)])
    return command


def checkpoint_command(
    *,
    height: int,
    data_dir: Path,
    chain_db: Path,
    stateroot_db: Path,
    checkpoint_root: Path,
    script: Path,
) -> list[str]:
    return [
        str(script),
        "none",
        "--once",
        "--height",
        str(height),
        "--data-dir",
        str(data_dir),
        "--chain-db",
        str(chain_db),
        "--stateroot-db",
        str(stateroot_db),
        "--root",
        str(checkpoint_root),
    ]


def build_plan(
    *,
    config: Path,
    node_bin: Path,
    rpc_url: str,
    milestones: list[int],
    poll_interval: float,
    max_seconds: float,
    chain_db: Path,
    stateroot_db: Path,
    probe_bin: Path,
    references: list[str],
    data_dir: Path,
    checkpoint_root: Path,
    checkpoint_script: Path,
    log_dir: Path,
    metrics_url: str | None = None,
) -> dict:
    steps = []
    for height in milestones:
        steps.append(
            {
                "height": height,
                "bounded_command": bounded_command(
                    config=config,
                    node_bin=node_bin,
                    rpc_url=rpc_url,
                    target_height=height,
                    poll_interval=poll_interval,
                    max_seconds=max_seconds,
                    chain_db=chain_db,
                    stateroot_db=stateroot_db,
                    probe_bin=probe_bin,
                    references=references,
                    node_output_log=log_dir / f"neo-node-milestone-h{height}.log",
                    metrics_url=metrics_url,
                ),
                "checkpoint_command": checkpoint_command(
                    height=height,
                    data_dir=data_dir,
                    chain_db=chain_db,
                    stateroot_db=stateroot_db,
                    checkpoint_root=checkpoint_root,
                    script=checkpoint_script,
                ),
            }
        )
    return {
        "mode": "dry-run",
        "config": str(config),
        "node_bin": str(node_bin),
        "probe_bin": str(probe_bin),
        "chain_db": str(chain_db),
        "stateroot_db": str(stateroot_db),
        "checkpoint_root": str(checkpoint_root),
        "milestones": milestones,
        "reference_rpcs": references,
        "metrics_url": metrics_url,
        "steps": steps,
    }


def run_command(command: list[str], runner: Callable[..., Any]) -> Any:
    return runner(
        command,
        check=False,
        capture_output=True,
        text=True,
    )


def height_sample_rate_summary(report: dict) -> dict:
    samples = report.get("height_samples") or []
    intervals: list[dict[str, Any]] = []
    previous: dict[str, Any] | None = None
    for sample in samples:
        if not isinstance(sample, dict):
            continue
        if previous is not None:
            try:
                from_elapsed = float(previous.get("elapsed_seconds"))
                to_elapsed = float(sample.get("elapsed_seconds"))
                from_height = int(previous.get("height"))
                to_height = int(sample.get("height"))
            except (TypeError, ValueError):
                previous = sample
                continue
            elapsed_delta = to_elapsed - from_elapsed
            height_delta = to_height - from_height
            if elapsed_delta > 0 and height_delta > 0:
                intervals.append(
                    {
                        "from_height": from_height,
                        "to_height": to_height,
                        "height_delta": height_delta,
                        "elapsed_seconds": elapsed_delta,
                        "blocks_per_second": height_delta / elapsed_delta,
                    }
                )
        previous = sample

    if not intervals:
        return {
            "sample_count": len([sample for sample in samples if isinstance(sample, dict)]),
            "interval_count": 0,
            "average_blocks_per_second": 0.0,
            "min_blocks_per_second": 0.0,
            "max_blocks_per_second": 0.0,
            "slowest_interval": None,
            "fastest_interval": None,
        }

    rates = [float(interval["blocks_per_second"]) for interval in intervals]
    slowest = min(intervals, key=lambda item: float(item["blocks_per_second"]))
    fastest = max(intervals, key=lambda item: float(item["blocks_per_second"]))
    return {
        "sample_count": len([sample for sample in samples if isinstance(sample, dict)]),
        "interval_count": len(intervals),
        "average_blocks_per_second": sum(rates) / len(rates),
        "min_blocks_per_second": min(rates),
        "max_blocks_per_second": max(rates),
        "slowest_interval": slowest,
        "fastest_interval": fastest,
    }


def metrics_sample_summary(report: dict) -> dict:
    samples = [sample for sample in report.get("height_samples") or [] if isinstance(sample, dict)]
    metrics_by_name: dict[str, list[float]] = {}
    metrics_error_count = 0
    for sample in samples:
        if sample.get("metrics_error"):
            metrics_error_count += 1
        metrics = sample.get("metrics") or {}
        if not isinstance(metrics, dict):
            continue
        for name, value in metrics.items():
            try:
                metrics_by_name.setdefault(str(name), []).append(float(value))
            except (TypeError, ValueError):
                continue

    return {
        "sample_count": len(samples),
        "metrics_error_count": metrics_error_count,
        "metrics": {
            name: {
                "sample_count": len(values),
                "first": values[0],
                "last": values[-1],
                "min": min(values),
                "max": max(values),
                "average": sum(values) / len(values),
            }
            for name, values in sorted(metrics_by_name.items())
            if values
        },
    }


def milestone_summary(result: dict) -> dict:
    report = result.get("bounded_report") or {}
    post_probe = report.get("post_probe") or {}
    root = post_probe.get("stateroot_root") or {}
    reference = post_probe.get("reference_stateroot") or {}
    return {
        "height": result.get("height"),
        "status": report.get("status"),
        "last_height": report.get("last_height"),
        "blocks_per_second": report.get("blocks_per_second", 0.0),
        "elapsed_seconds": report.get("elapsed_seconds", 0.0),
        "stateroot_matches_chain": post_probe.get("stateroot_matches_chain"),
        "reference_matches_local": reference.get("matches_local"),
        "successful_reference_samples": reference.get("successful_samples", 0),
        "local_root": root.get("root"),
        "checkpoint_created": result.get("checkpoint_returncode") == 0,
        "height_sample_rate_summary": height_sample_rate_summary(report),
        "metrics_sample_summary": metrics_sample_summary(report),
    }


def build_run_summary(plan: dict, results: list[dict], mode: str) -> dict:
    milestones = [milestone_summary(result) for result in results]
    completed = [item for item in milestones if item.get("checkpoint_created")]
    bps_values = [
        float(item["blocks_per_second"])
        for item in milestones
        if item.get("blocks_per_second") is not None
    ]
    latest = completed[-1] if completed else (milestones[-1] if milestones else {})
    return {
        "mode": mode,
        "requested_milestones": plan["milestones"],
        "completed_heights": [item["height"] for item in completed],
        "latest_height": latest.get("last_height"),
        "latest_root": latest.get("local_root"),
        "average_blocks_per_second": sum(bps_values) / len(bps_values) if bps_values else 0.0,
        "all_reference_matched": all(
            item.get("reference_matches_local") is True for item in completed
        )
        if completed
        else False,
        "milestones": milestones,
    }


def summary_history_record(plan: dict, result: dict) -> dict:
    return {
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "mode": result["mode"],
        "config": plan["config"],
        "node_bin": plan.get("node_bin"),
        "probe_bin": plan.get("probe_bin"),
        "chain_db": plan["chain_db"],
        "stateroot_db": plan["stateroot_db"],
        "checkpoint_root": plan["checkpoint_root"],
        "summary": result.get("summary", {}),
    }


def append_summary_jsonl(path: Path, record: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, sort_keys=True))
        handle.write("\n")


def run_milestones(
    plan: dict,
    *,
    runner: Callable[..., Any] = subprocess.run,
    include_command_output: bool = False,
) -> dict:
    results = []
    for step in plan["steps"]:
        bounded = run_command(step["bounded_command"], runner)
        bounded_report = None
        if getattr(bounded, "stdout", ""):
            try:
                bounded_report = parse_last_json_object(bounded.stdout)
            except ValueError as exc:
                bounded_report = {"parse_error": str(exc)}
        result = {
            "height": step["height"],
            "bounded_command": step["bounded_command"],
            "bounded_returncode": bounded.returncode,
            "bounded_report": bounded_report,
        }
        bounded_failed = bounded.returncode != 0 or (
            bounded_report or {}
        ).get("status") != "target-reached"
        if include_command_output or bounded_failed:
            result["bounded_stdout"] = getattr(bounded, "stdout", "") or ""
            result["bounded_stderr"] = getattr(bounded, "stderr", "") or ""
        results.append(result)
        if bounded_failed:
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "bounded-replay",
                "summary": build_run_summary(plan, results, "failed"),
                "results": results,
            }

        checkpoint = run_command(step["checkpoint_command"], runner)
        result["checkpoint_command"] = step["checkpoint_command"]
        result["checkpoint_returncode"] = checkpoint.returncode
        if include_command_output or checkpoint.returncode != 0:
            result["checkpoint_stdout"] = getattr(checkpoint, "stdout", "") or ""
            result["checkpoint_stderr"] = getattr(checkpoint, "stderr", "") or ""
        if checkpoint.returncode != 0:
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "checkpoint",
                "summary": build_run_summary(plan, results, "failed"),
                "results": results,
            }

    return {
        "mode": "completed",
        "milestones": plan["milestones"],
        "checkpoint_root": plan["checkpoint_root"],
        "summary": build_run_summary(plan, results, "completed"),
        "results": results,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run sequential StateRoot bounded replay milestones, compare each "
            "local root with reference RPCs, and checkpoint each successful height."
        )
    )
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--node-bin", default=DEFAULT_NODE_BIN, type=Path)
    parser.add_argument("--rpc", default=DEFAULT_RPC)
    parser.add_argument(
        "--milestone",
        action="append",
        default=[],
        help="Milestone height. Repeat the flag or pass comma-separated heights.",
    )
    parser.add_argument("--poll-interval", default=5.0, type=float)
    parser.add_argument("--max-seconds", default=900.0, type=float)
    parser.add_argument("--chain-db", required=True, type=Path)
    parser.add_argument("--stateroot-db", required=True, type=Path)
    parser.add_argument("--probe-bin", default=DEFAULT_PROBE_BIN, type=Path)
    parser.add_argument(
        "--reference",
        action="append",
        default=[],
        help="Reference RPC URL(s). Repeat the flag or pass comma-separated URLs.",
    )
    parser.add_argument("--data-dir", default=None, type=Path)
    parser.add_argument("--checkpoint-root", required=True, type=Path)
    parser.add_argument(
        "--checkpoint-script",
        default=Path("scripts/checkpoint-on-height.sh"),
        type=Path,
    )
    parser.add_argument("--log-dir", required=True, type=Path)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument(
        "--include-command-output",
        action="store_true",
        help="Include raw child stdout/stderr in the final JSON even for successful steps.",
    )
    parser.add_argument(
        "--summary-jsonl",
        default=None,
        type=Path,
        help="Append one compact run summary JSON object per line for performance history.",
    )
    parser.add_argument(
        "--metrics-url",
        default=None,
        help="Optional Prometheus /metrics URL to sample during each bounded replay.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        milestones = parse_height_values(args.milestone)
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    references = normalize_reference_urls(args.reference) or DEFAULT_REFERENCE_RPCS
    data_dir = args.data_dir or args.chain_db.parent
    plan = build_plan(
        config=args.config,
        node_bin=args.node_bin,
        rpc_url=args.rpc,
        milestones=milestones,
        poll_interval=args.poll_interval,
        max_seconds=args.max_seconds,
        chain_db=args.chain_db,
        stateroot_db=args.stateroot_db,
        probe_bin=args.probe_bin,
        references=references,
        data_dir=data_dir,
        checkpoint_root=args.checkpoint_root,
        checkpoint_script=args.checkpoint_script,
        log_dir=args.log_dir,
        metrics_url=args.metrics_url,
    )
    if args.dry_run:
        print(json.dumps(plan, indent=2, sort_keys=True))
        return 0

    args.log_dir.mkdir(parents=True, exist_ok=True)
    result = run_milestones(plan, include_command_output=args.include_command_output)
    if args.summary_jsonl is not None:
        append_summary_jsonl(args.summary_jsonl, summary_history_record(plan, result))
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["mode"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
