#!/usr/bin/env python3
"""Run a bounded neo-node replay until a target height, then stop it."""

from __future__ import annotations

import argparse
import contextlib
import json
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any, Callable


DEFAULT_REFERENCE_RPCS = [
    "http://seed1.neo.org:10332",
    "http://seed2.neo.org:10332",
    "http://seed3.neo.org:10332",
    "http://seed4.neo.org:10332",
    "http://seed5.neo.org:10332",
]
DEFAULT_METRIC_NAMES = [
    "neo_sync_avg_total_us",
    "neo_sync_avg_verify_us",
    "neo_sync_avg_persist_us",
    "neo_sync_avg_commit_us",
    "neo_sync_blocks_persisted",
    "neo_sync_native_persist_blocks_total",
    "neo_sync_native_persist_height",
    "neo_sync_native_persist_avg_total_us",
    "neo_sync_native_persist_avg_onpersist_us",
    "neo_sync_native_persist_avg_tx_us",
    "neo_sync_native_persist_avg_postpersist_us",
    "neo_sync_native_persist_avg_cache_commit_us",
    "neo_sync_native_persist_avg_tx_count",
    "neo_sync_native_contract_hook_calls_total",
    "neo_sync_native_contract_hook_avg_us",
    "neo_sync_neotoken_onpersist_stage_calls_total",
    "neo_sync_neotoken_onpersist_stage_avg_us",
    "neo_sync_neotoken_committee_compute_stage_calls_total",
    "neo_sync_neotoken_committee_compute_stage_avg_us",
    "neo_sync_neotoken_committee_candidate_scan_samples_total",
    "neo_sync_neotoken_committee_candidate_scan_items_total",
    "neo_sync_neotoken_committee_candidate_scan_avg_items",
    "neo_state_service_mpt_apply_blocks_total",
    "neo_state_service_mpt_apply_failures_total",
    "neo_state_service_mpt_apply_avg_total_us",
    "neo_state_service_mpt_apply_avg_project_us",
    "neo_state_service_mpt_apply_avg_trie_us",
    "neo_state_service_mpt_apply_avg_changes",
    "neo_state_service_mpt_apply_stage_calls_total",
    "neo_state_service_mpt_apply_stage_avg_us",
    "neo_state_service_mpt_apply_count_samples_total",
    "neo_state_service_mpt_apply_items_total",
    "neo_state_service_mpt_apply_avg_items",
]


class SystemClock:
    def time(self) -> float:
        return time.time()

    def sleep(self, seconds: float) -> None:
        time.sleep(seconds)


def rpc_call(url: str, method: str, params: list | None = None, timeout: float = 5.0) -> Any:
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or []}
    ).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        data = json.loads(response.read().decode("utf-8"))
    if data.get("error"):
        raise RuntimeError(f"{method} returned error from {url}: {data['error']}")
    return data["result"]


def parse_prometheus_metrics(
    text: str,
    metric_names: list[str] | None = None,
) -> dict[str, float]:
    wanted = set(metric_names or DEFAULT_METRIC_NAMES)
    metrics: dict[str, float] = {}
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) < 2:
            continue
        name = parts[0].split("{", 1)[0]
        if name not in wanted:
            continue
        try:
            metric_key = parts[0] if "{" in parts[0] else name
            metrics[metric_key] = float(parts[1])
        except ValueError:
            continue
    return metrics


def fetch_prometheus_metrics(
    url: str,
    timeout: float = 2.0,
    metric_names: list[str] | None = None,
) -> dict[str, float]:
    request = urllib.request.Request(url, method="GET")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        text = response.read().decode("utf-8")
    return parse_prometheus_metrics(text, metric_names)


def stop_process(process: Any) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate(timeout=20)


def node_command(node_bin: Path, config: Path, target_height: int) -> list[str]:
    return [
        str(node_bin),
        "--config",
        str(config),
        "--stop-at-height",
        str(target_height),
    ]


def read_probe_ledger_height(db_path: Path, probe_bin: Path) -> int | None:
    completed = subprocess.run(
        [
            str(probe_bin),
            "--db",
            str(db_path),
            "--contract-id",
            "-4",
            "--key-hex",
            "0c",
            "--decode",
            "hash-index",
        ],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    payload = json.loads(completed.stdout)
    decoded = payload.get("decoded") or {}
    if "index" not in decoded:
        return None
    return int(decoded["index"])


def read_probe_mpt_state_height(db_path: Path, probe_bin: Path) -> int | None:
    completed = subprocess.run(
        [
            str(probe_bin),
            "--db",
            str(db_path),
            "--mpt-state-height",
        ],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    payload = json.loads(completed.stdout)
    height = payload.get("height") or {}
    decoded = height.get("decoded") or {}
    if "current_local_root_index" not in decoded:
        return None
    return int(decoded["current_local_root_index"])


def read_probe_mpt_state_root(db_path: Path, probe_bin: Path, index: int) -> str | None:
    completed = subprocess.run(
        [
            str(probe_bin),
            "--db",
            str(db_path),
            "--mpt-state-root",
            str(index),
        ],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    payload = json.loads(completed.stdout)
    state_root = payload.get("state_root") or {}
    decoded = state_root.get("decoded") or {}
    return decoded.get("roothash")


def normalize_reference_urls(values: list[str] | None) -> list[str]:
    urls: list[str] = []
    for value in values or []:
        for part in str(value).split(","):
            part = part.strip()
            if part:
                urls.append(part)
    return urls


def fetch_reference_stateroots(
    *,
    reference_urls: list[str],
    index: int,
    local_root: str | None,
    rpc: Callable[..., Any] = rpc_call,
) -> dict:
    samples: list[dict[str, Any]] = []
    roots: list[str] = []
    for url in reference_urls:
        try:
            result = rpc(url, "getstateroot", [index], 10.0)
            root = result.get("roothash") if isinstance(result, dict) else None
            sample = {
                "url": url,
                "ok": root is not None,
                "root": root,
            }
            if root is None:
                sample["error"] = f"missing roothash in getstateroot({index}) response"
            else:
                roots.append(str(root))
        except Exception as exc:  # pylint: disable=broad-except
            sample = {
                "url": url,
                "ok": False,
                "error": str(exc),
            }
        samples.append(sample)

    unique_roots = sorted(set(roots))
    matches_local = bool(local_root and roots and all(root == local_root for root in roots))
    return {
        "index": index,
        "local_root": local_root,
        "reference_roots": unique_roots,
        "matches_local": matches_local,
        "successful_samples": len(roots),
        "sample_count": len(samples),
        "samples": samples,
    }


def collect_post_probe(
    *,
    chain_db: Path | None,
    stateroot_db: Path | None,
    probe_bin: Path,
    reference_urls: list[str] | None = None,
    rpc: Callable[..., Any] = rpc_call,
) -> dict:
    post_probe: dict[str, Any] = {}
    state_height: int | None = None
    local_root: str | None = None

    if chain_db is not None:
        try:
            chain_height = read_probe_ledger_height(chain_db, probe_bin)
            post_probe["chain_height"] = {
                "db": str(chain_db),
                "height": chain_height,
                "found": chain_height is not None,
                "ok": chain_height is not None,
            }
        except Exception as exc:  # pylint: disable=broad-except
            post_probe["chain_height"] = {
                "ok": False,
                "db": str(chain_db),
                "error": str(exc),
            }

    if stateroot_db is not None:
        try:
            state_height = read_probe_mpt_state_height(stateroot_db, probe_bin)
            post_probe["stateroot_height"] = {
                "ok": state_height is not None,
                "db": str(stateroot_db),
                "height": state_height,
                "found": state_height is not None,
            }
        except Exception as exc:  # pylint: disable=broad-except
            post_probe["stateroot_height"] = {
                "ok": False,
                "db": str(stateroot_db),
                "error": str(exc),
            }
        if state_height is not None:
            try:
                local_root = read_probe_mpt_state_root(stateroot_db, probe_bin, state_height)
                post_probe["stateroot_root"] = {
                    "ok": local_root is not None,
                    "db": str(stateroot_db),
                    "height": state_height,
                    "root": local_root,
                    "found": local_root is not None,
                }
            except Exception as exc:  # pylint: disable=broad-except
                post_probe["stateroot_root"] = {
                    "ok": False,
                    "db": str(stateroot_db),
                    "height": state_height,
                    "error": str(exc),
                }

    chain_height = (post_probe.get("chain_height") or {}).get("height")
    state_height = (post_probe.get("stateroot_height") or {}).get("height")
    if chain_height is not None and state_height is not None:
        post_probe["stateroot_matches_chain"] = int(chain_height) == int(state_height)
    elif chain_db is not None and stateroot_db is not None:
        post_probe["stateroot_matches_chain"] = False

    references = normalize_reference_urls(reference_urls)
    if references and state_height is not None:
        post_probe["reference_stateroot"] = fetch_reference_stateroots(
            reference_urls=references,
            index=state_height,
            local_root=local_root,
            rpc=rpc,
        )

    return post_probe


def attach_post_probe_report(
    report: dict,
    *,
    chain_db: Path | None,
    stateroot_db: Path | None,
    probe_bin: Path,
    require_stateroot_height_match: bool,
    reference_urls: list[str] | None = None,
    require_reference_stateroot_match: bool = False,
    rpc: Callable[..., Any] = rpc_call,
) -> dict:
    if (
        chain_db is None
        and stateroot_db is None
        and not reference_urls
        and not require_reference_stateroot_match
    ):
        return report

    effective_reference_urls = reference_urls
    if require_reference_stateroot_match and not normalize_reference_urls(reference_urls):
        effective_reference_urls = DEFAULT_REFERENCE_RPCS

    post_probe = collect_post_probe(
        chain_db=chain_db,
        stateroot_db=stateroot_db,
        probe_bin=probe_bin,
        reference_urls=effective_reference_urls,
        rpc=rpc,
    )
    report["post_probe"] = post_probe
    if (
        require_stateroot_height_match
        and report.get("status") == "target-reached"
        and post_probe.get("stateroot_matches_chain") is not True
    ):
        report["status"] = "stateroot-height-mismatch"
    if (
        require_reference_stateroot_match
        and report.get("status") == "target-reached"
        and (post_probe.get("reference_stateroot") or {}).get("matches_local") is not True
    ):
        report["status"] = "reference-stateroot-mismatch"
    return report


def classify_process_exit(
    *,
    returncode: int | None,
    last_height: int | None,
    target_height: int,
) -> str:
    if returncode == 0 and last_height is not None and last_height >= target_height:
        return "target-reached"
    return "process-exited"


def run_until_target(
    *,
    command: list[str],
    rpc_url: str,
    target_height: int,
    poll_interval: float,
    max_seconds: float,
    spawner: Callable[..., Any] = subprocess.Popen,
    rpc: Callable[..., Any] = rpc_call,
    clock: Any | None = None,
    repairable_failure_detector: Callable[[], bool] | None = None,
    height_reader: Callable[[], int | None] | None = None,
    node_output: Any | None = None,
    metrics_url: str | None = None,
    metrics_fetcher: Callable[[str], dict[str, float]] = fetch_prometheus_metrics,
) -> dict:
    clock = clock or SystemClock()
    started_at = clock.time()
    process = spawner(
        command,
        stdout=node_output if node_output is not None else subprocess.DEVNULL,
        stderr=subprocess.STDOUT,
        text=True,
    )
    samples: list[dict] = []
    last_height: int | None = None
    status = "timeout"

    def read_height_from_fallback() -> tuple[int | None, str | None]:
        if height_reader is None:
            return None, None
        height = height_reader()
        if height is None:
            return None, None
        return int(height), "fallback"

    def attach_metrics(sample: dict) -> None:
        if not metrics_url:
            return
        try:
            sample["metrics"] = metrics_fetcher(metrics_url)
        except Exception as exc:  # pylint: disable=broad-except
            sample["metrics_error"] = str(exc)

    try:
        while clock.time() - started_at < max_seconds:
            if process.poll() is not None:
                if process.returncode == 0 and (
                    last_height is None or last_height < target_height
                ):
                    try:
                        block_count = int(rpc(rpc_url, "getblockcount", [], 5.0))
                        last_height = block_count - 1
                    except Exception:  # pylint: disable=broad-except
                        try:
                            height, _source = read_height_from_fallback()
                            if height is not None:
                                last_height = height
                        except Exception:  # pylint: disable=broad-except
                            pass
                status = classify_process_exit(
                    returncode=process.returncode,
                    last_height=last_height,
                    target_height=target_height,
                )
                break
            try:
                block_count = int(rpc(rpc_url, "getblockcount", [], 5.0))
                height = block_count - 1
                last_height = height
                sample = {
                    "elapsed_seconds": round(clock.time() - started_at, 3),
                    "height": height,
                    "height_source": "rpc",
                }
                attach_metrics(sample)
                samples.append(sample)
                print(json.dumps(sample), flush=True)
                if height >= target_height:
                    status = "target-reached"
                    break
                if repairable_failure_detector is not None and repairable_failure_detector():
                    status = "repairable-failure"
                    break
            except Exception as exc:  # pylint: disable=broad-except
                sample = {
                    "elapsed_seconds": round(clock.time() - started_at, 3),
                    "waiting_rpc": str(exc),
                }
                try:
                    height, source = read_height_from_fallback()
                    if height is not None:
                        last_height = height
                        sample["height"] = height
                        sample["height_source"] = source
                except Exception as height_exc:  # pylint: disable=broad-except
                    sample["height_reader_error"] = str(height_exc)
                attach_metrics(sample)
                samples.append(sample)
                print(json.dumps(sample), flush=True)
                if last_height is not None and last_height >= target_height:
                    status = "target-reached"
                    break
                if repairable_failure_detector is not None and repairable_failure_detector():
                    status = "repairable-failure"
                    break
            clock.sleep(poll_interval)
    finally:
        stop_process(process)

    if height_reader is not None and (last_height is None or last_height < target_height):
        try:
            height, _source = read_height_from_fallback()
            if height is not None:
                last_height = height
        except Exception:  # pylint: disable=broad-except
            pass

    elapsed = max(clock.time() - started_at, 0.0)
    first_height = next(
        (sample["height"] for sample in samples if "height" in sample),
        None,
    )
    if first_height is not None and last_height is not None and elapsed > 0:
        blocks_per_second = max(last_height - first_height, 0) / elapsed
    else:
        blocks_per_second = 0.0

    return {
        "command": command,
        "pid": int(process.pid),
        "status": status,
        "target_height": target_height,
        "last_height": last_height,
        "elapsed_seconds": round(elapsed, 3),
        "blocks_per_second": blocks_per_second,
        "height_samples": samples,
        "returncode": process.returncode,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run neo-node until a target block height and emit a JSON replay summary."
    )
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--node-bin", default=Path("target/release/neo-node"), type=Path)
    parser.add_argument("--rpc", default="http://127.0.0.1:21332")
    parser.add_argument("--target-height", required=True, type=int)
    parser.add_argument("--poll-interval", default=30.0, type=float)
    parser.add_argument("--max-seconds", default=900.0, type=float)
    parser.add_argument("--db", default=None, type=Path)
    parser.add_argument("--probe-bin", default=Path("target/release/neo-db-probe"), type=Path)
    parser.add_argument(
        "--stateroot-db",
        default=None,
        type=Path,
        help="Optional StateService MPT RocksDB path to probe after the node stops.",
    )
    parser.add_argument(
        "--require-stateroot-height-match",
        action="store_true",
        help="Fail a target-reached run unless probed chain and StateService MPT heights match.",
    )
    parser.add_argument(
        "--reference",
        action="append",
        default=[],
        help=(
            "Reference RPC URL(s) for post-run getstateroot comparison. "
            "Repeat the flag or pass comma-separated URLs."
        ),
    )
    parser.add_argument(
        "--require-reference-stateroot-match",
        action="store_true",
        help="Fail a target-reached run unless the local MPT root matches a reference RPC root.",
    )
    parser.add_argument(
        "--node-output-log",
        default=None,
        type=Path,
        help="Append neo-node stdout/stderr to this file while replaying.",
    )
    parser.add_argument(
        "--metrics-url",
        default=None,
        help="Optional Prometheus /metrics URL to sample alongside height polls.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    height_reader = None
    if args.db is not None:
        height_reader = lambda: read_probe_ledger_height(args.db, args.probe_bin)
    if args.node_output_log is not None:
        args.node_output_log.parent.mkdir(parents=True, exist_ok=True)
    with (
        args.node_output_log.open("a", encoding="utf-8")
        if args.node_output_log is not None
        else contextlib.nullcontext(None)
    ) as node_output:
        report = run_until_target(
            command=node_command(args.node_bin, args.config, args.target_height),
            rpc_url=args.rpc,
            target_height=args.target_height,
            poll_interval=args.poll_interval,
            max_seconds=args.max_seconds,
            height_reader=height_reader,
            node_output=node_output,
            metrics_url=args.metrics_url,
        )
    report = attach_post_probe_report(
        report,
        chain_db=args.db,
        stateroot_db=args.stateroot_db,
        probe_bin=args.probe_bin,
        require_stateroot_height_match=args.require_stateroot_height_match,
        reference_urls=args.reference,
        require_reference_stateroot_match=args.require_reference_stateroot_match,
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    if report["status"] == "target-reached":
        return 0
    if report["status"] == "timeout":
        return 124
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
