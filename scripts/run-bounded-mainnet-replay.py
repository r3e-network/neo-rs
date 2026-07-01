#!/usr/bin/env python3
"""Run a bounded neo-node replay until a target height, then stop it."""

from __future__ import annotations

import argparse
import contextlib
import json
import ipaddress
import subprocess
import sys
import time
import urllib.parse
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
    "neo_sync_native_persist_tx_stage_calls_total",
    "neo_sync_native_persist_tx_stage_avg_us",
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
    "neo_storage_rocksdb_batch_pending_operations",
    "neo_storage_rocksdb_batch_batches_flushed_total",
    "neo_storage_rocksdb_batch_operations_written_total",
    "neo_storage_rocksdb_batch_bytes_written_total",
    "neo_storage_rocksdb_batch_flush_timeouts_total",
    "neo_storage_rocksdb_batch_avg_ops_per_flush",
    "neo_storage_rocksdb_batch_avg_bytes_per_flush",
    "neo_storage_rocksdb_batch_avg_flush_duration_ms",
    "neo_storage_rocksdb_batch_max_batch_size",
    "neo_storage_rocksdb_batch_max_batch_bytes",
    "neo_storage_rocksdb_batch_disable_wal",
]
DEFAULT_POLL_INTERVAL_SECONDS = 30.0
DEFAULT_IMPORT_POLL_INTERVAL_SECONDS = 1.0
DEFAULT_SYNC_SPEED_FLOOR_BPS = 1500.0
TRANSACTION_WORK_METRIC_NAMES = {
    "neo_sync_native_persist_avg_tx_count",
}


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
    with open_http_request(request, timeout=timeout) as response:
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
    with open_http_request(request, timeout=timeout) as response:
        text = response.read().decode("utf-8")
    return parse_prometheus_metrics(text, metric_names)


def is_loopback_url(url: str) -> bool:
    hostname = urllib.parse.urlparse(url).hostname
    if hostname is None:
        return False
    if hostname.lower() == "localhost":
        return True
    try:
        return ipaddress.ip_address(hostname).is_loopback
    except ValueError:
        return False


def open_http_request(request: urllib.request.Request, timeout: float):
    if is_loopback_url(request.full_url):
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        return opener.open(request, timeout=timeout)
    return urllib.request.urlopen(request, timeout=timeout)


def stop_process(process: Any) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate(timeout=20)


def fast_sync_cache_progress(cache_dir: Path) -> dict[str, Any]:
    zip_files = sorted(cache_dir.glob("*.zip"))
    if zip_files:
        latest_zip = max(zip_files, key=lambda path: path.stat().st_mtime)
        acc_files = sorted(path for path in cache_dir.rglob("*.acc") if path.is_file())
        progress = {
            "fast_sync_stage": "downloaded",
            "fast_sync_package_path": latest_zip.name,
            "fast_sync_package_bytes": latest_zip.stat().st_size,
        }
        if acc_files:
            latest_acc = max(acc_files, key=lambda path: path.stat().st_mtime)
            progress.update(
                {
                    "fast_sync_stage": "extracted",
                    "fast_sync_chain_path": str(latest_acc.relative_to(cache_dir)),
                    "fast_sync_chain_bytes": latest_acc.stat().st_size,
                }
            )
        return progress
    partial_files = sorted(cache_dir.glob("*.zip.part"))
    if partial_files:
        latest_partial = max(partial_files, key=lambda path: path.stat().st_mtime)
        return {
            "fast_sync_stage": "downloading",
            "fast_sync_partial_path": latest_partial.name,
            "fast_sync_partial_bytes": latest_partial.stat().st_size,
        }
    return {"fast_sync_stage": "waiting-for-download"}


def node_command(
    node_bin: Path,
    config: Path,
    target_height: int,
    import_chain: Path | None = None,
    fast_sync: bool = False,
    fast_sync_cache: Path | None = None,
    fast_sync_report: Path | None = None,
) -> list[str]:
    if import_chain is not None and fast_sync:
        raise ValueError("cannot combine --import-chain with --fast-sync")
    if fast_sync_cache is not None and not fast_sync:
        raise ValueError("--fast-sync-cache requires --fast-sync")

    command = [
        str(node_bin),
        "--config",
        str(config),
        "--stop-at-height",
        str(target_height),
    ]
    if import_chain is not None:
        command.extend(["--import-chain", str(import_chain)])
    if fast_sync:
        command.append("--fast-sync")
        if fast_sync_cache is not None:
            command.extend(["--fast-sync-cache", str(fast_sync_cache)])
        if fast_sync_report is not None:
            command.extend(["--fast-sync-report", str(fast_sync_report)])
    return command


DEFAULT_STORAGE_PROVIDER = "mdbx"


def probe_command_prefix(probe_bin: Path, db_path: Path, storage_provider: str) -> list[str]:
    return [
        str(probe_bin),
        "--db",
        str(db_path),
        "--storage-provider",
        storage_provider,
    ]


def read_probe_ledger_height(
    db_path: Path,
    probe_bin: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> int | None:
    completed = subprocess.run(
        [
            *probe_command_prefix(probe_bin, db_path, storage_provider),
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


def read_probe_mpt_state_height(
    db_path: Path,
    probe_bin: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> int | None:
    completed = subprocess.run(
        [
            *probe_command_prefix(probe_bin, db_path, storage_provider),
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


def read_probe_mpt_state_root(
    db_path: Path,
    probe_bin: Path,
    index: int,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> str | None:
    completed = subprocess.run(
        [
            *probe_command_prefix(probe_bin, db_path, storage_provider),
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
    all_references_succeeded = bool(reference_urls) and len(roots) == len(reference_urls)
    matches_local = bool(
        local_root
        and all_references_succeeded
        and all(root == local_root for root in roots)
    )
    return {
        "index": index,
        "local_root": local_root,
        "reference_roots": unique_roots,
        "matches_local": matches_local,
        "all_references_succeeded": all_references_succeeded,
        "successful_samples": len(roots),
        "sample_count": len(samples),
        "samples": samples,
    }


def reference_stateroot_match_is_strong(reference: dict, expected_count: int) -> bool:
    if reference.get("matches_local") is not True:
        return False
    try:
        successful_samples = int(reference.get("successful_samples"))
        sample_count = int(reference.get("sample_count"))
    except (TypeError, ValueError):
        return False
    return successful_samples >= expected_count and sample_count >= expected_count


def collect_post_probe(
    *,
    chain_db: Path | None,
    stateroot_db: Path | None,
    probe_bin: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
    reference_urls: list[str] | None = None,
    rpc: Callable[..., Any] = rpc_call,
) -> dict:
    post_probe: dict[str, Any] = {}
    state_height: int | None = None
    local_root: str | None = None

    if chain_db is not None:
        try:
            chain_height = read_probe_ledger_height(chain_db, probe_bin, storage_provider)
            post_probe["chain_height"] = {
                "db": str(chain_db),
                "storage_provider": storage_provider,
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
            state_height = read_probe_mpt_state_height(stateroot_db, probe_bin, storage_provider)
            post_probe["stateroot_height"] = {
                "ok": state_height is not None,
                "db": str(stateroot_db),
                "storage_provider": storage_provider,
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
                local_root = read_probe_mpt_state_root(
                    stateroot_db,
                    probe_bin,
                    state_height,
                    storage_provider,
                )
                post_probe["stateroot_root"] = {
                    "ok": local_root is not None,
                    "db": str(stateroot_db),
                    "storage_provider": storage_provider,
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
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
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
        storage_provider=storage_provider,
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
        and not reference_stateroot_match_is_strong(
            post_probe.get("reference_stateroot") or {},
            len(normalize_reference_urls(effective_reference_urls)),
        )
    ):
        report["status"] = "reference-stateroot-mismatch"
    report["sync_proof"] = build_sync_proof(report)
    return report


def attach_fast_sync_report(report: dict, path: Path | None) -> dict:
    if report.get("sync_source") != "fast-sync":
        report["sync_proof"] = build_sync_proof(report)
        return report
    if path is None or not path.is_file():
        report["fast_sync_report_error"] = (
            "fast-sync report sidecar is missing; node did not produce the required import proof"
        )
        if report.get("status") == "target-reached":
            report["status"] = "fast-sync-report-missing"
        report["sync_proof"] = build_sync_proof(report)
        return report
    try:
        fast_sync_report = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:  # pylint: disable=broad-except
        report["fast_sync_report_error"] = str(exc)
        if report.get("status") == "target-reached":
            report["status"] = "fast-sync-report-invalid"
        report["sync_proof"] = build_sync_proof(report)
        return report
    if not isinstance(fast_sync_report, dict):
        report["fast_sync_report_error"] = "fast-sync report is not a JSON object"
        if report.get("status") == "target-reached":
            report["status"] = "fast-sync-report-invalid"
        report["sync_proof"] = build_sync_proof(report)
        return report
    report["fast_sync_report"] = fast_sync_report
    report["sync_proof"] = build_sync_proof(report)
    return report


def stateroot_covers_observed_chain_height(
    *,
    target_height: int,
    observed_chain_height: int | None,
    stateroot_height: int | None,
) -> bool:
    if stateroot_height is None:
        return False
    required_height = (
        int(observed_chain_height)
        if observed_chain_height is not None
        else int(target_height)
    )
    return int(stateroot_height) >= required_height


def classify_process_exit(
    *,
    returncode: int | None,
    last_height: int | None,
    target_height: int,
    target_ready: bool = True,
) -> str:
    if (
        returncode == 0
        and last_height is not None
        and last_height >= target_height
        and target_ready
    ):
        return "target-reached"
    return "process-exited"


def metrics_sample_count(samples: list[dict]) -> int:
    count = 0
    for sample in samples:
        metrics = sample.get("metrics") if isinstance(sample, dict) else None
        if isinstance(metrics, dict) and metrics:
            count += 1
    return count


def summarize_metric_samples(samples: list[dict]) -> dict[str, Any]:
    metrics_by_name: dict[str, list[float]] = {}
    metrics_error_count = 0
    for sample in samples:
        if not isinstance(sample, dict):
            continue
        if sample.get("metrics_error"):
            metrics_error_count += 1
        metrics = sample.get("metrics")
        if not isinstance(metrics, dict):
            continue
        for name, value in metrics.items():
            try:
                metrics_by_name.setdefault(str(name), []).append(float(value))
            except (TypeError, ValueError):
                continue

    metrics = {
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
    }
    return {
        "sample_count": sum(1 for sample in samples if isinstance(sample, dict)),
        "metrics_error_count": metrics_error_count,
        "metrics": metrics,
        "hot_metrics_by_average_us": hot_metrics_by_average_us(metrics),
        "hot_count_metrics_by_average": hot_count_metrics_by_average(metrics),
    }


def transaction_work_summary(metrics_summary: dict[str, Any]) -> dict[str, Any]:
    metrics = metrics_summary.get("metrics")
    if not isinstance(metrics, dict):
        metrics = {}
    tx_metrics: list[dict[str, Any]] = []
    observed_transaction_work = False
    for name, stats in sorted(metrics.items()):
        metric_name = str(name).split("{", 1)[0]
        if metric_name not in TRANSACTION_WORK_METRIC_NAMES or not isinstance(stats, dict):
            continue
        try:
            max_value = float(stats["max"])
            average_value = float(stats["average"])
            last_value = float(stats["last"])
            sample_count = int(stats.get("sample_count", 0))
        except (KeyError, TypeError, ValueError):
            continue
        observed_transaction_work = observed_transaction_work or max_value > 0.0
        tx_metrics.append(
            {
                "name": str(name),
                "sample_count": sample_count,
                "average": average_value,
                "last": last_value,
                "max": max_value,
                "observed_transaction_work": max_value > 0.0,
            }
        )
    return {
        "required_for_speed_proof": True,
        "observed_transaction_work": observed_transaction_work,
        "metric_count": len(tx_metrics),
        "metrics": tx_metrics,
    }


def hot_metrics_by_average_us(metrics: dict[str, dict], limit: int = 8) -> list[dict[str, Any]]:
    hot = []
    for name, stats in metrics.items():
        metric_name = name.split("{", 1)[0]
        if not metric_name.endswith("_us"):
            continue
        try:
            hot.append(
                {
                    "name": name,
                    "average_us": float(stats["average"]),
                    "last_us": float(stats["last"]),
                    "max_us": float(stats["max"]),
                    "sample_count": int(stats.get("sample_count", 0)),
                }
            )
        except (KeyError, TypeError, ValueError):
            continue
    hot.sort(key=lambda item: (-item["average_us"], item["name"]))
    return hot[:limit]


def hot_count_metrics_by_average(metrics: dict[str, dict], limit: int = 8) -> list[dict[str, Any]]:
    hot = []
    for name, stats in metrics.items():
        metric_name = name.split("{", 1)[0]
        if not (
            metric_name.endswith("_items")
            or metric_name.endswith("_avg_items")
            or metric_name.endswith("_avg_changes")
            or metric_name.endswith("_avg_tx_count")
        ):
            continue
        try:
            average = float(stats["average"])
            if average <= 0:
                continue
            hot.append(
                {
                    "name": name,
                    "average": average,
                    "last": float(stats["last"]),
                    "max": float(stats["max"]),
                    "sample_count": int(stats.get("sample_count", 0)),
                }
            )
        except (KeyError, TypeError, ValueError):
            continue
    hot.sort(key=lambda item: (-item["average"], item["name"]))
    return hot[:limit]


def ordered_sample_sources(samples: list[dict]) -> list[str]:
    sources: list[str] = []
    for sample in samples:
        if not isinstance(sample, dict):
            continue
        source = sample.get("height_source")
        if source is None:
            continue
        source_text = str(source)
        if source_text not in sources:
            sources.append(source_text)
    return sources


def latest_fast_sync_cache_proof(samples: list[dict]) -> dict[str, Any] | None:
    proof: dict[str, Any] = {}
    for sample in samples:
        if not isinstance(sample, dict):
            continue
        if "fast_sync_stage" in sample:
            proof["stage"] = sample["fast_sync_stage"]
        if "fast_sync_package_path" in sample:
            proof["package_path"] = sample["fast_sync_package_path"]
        if "fast_sync_package_bytes" in sample:
            proof["package_bytes"] = sample["fast_sync_package_bytes"]
        if "fast_sync_chain_path" in sample:
            proof["chain_path"] = sample["fast_sync_chain_path"]
        if "fast_sync_chain_bytes" in sample:
            proof["chain_bytes"] = sample["fast_sync_chain_bytes"]
        if "fast_sync_partial_path" in sample:
            proof["partial_path"] = sample["fast_sync_partial_path"]
        if "fast_sync_partial_bytes" in sample:
            proof["partial_bytes"] = sample["fast_sync_partial_bytes"]
        if "progress_error" in sample:
            proof["progress_error"] = sample["progress_error"]
    return proof or None


def sync_post_probe_proof(report: dict) -> dict[str, Any] | None:
    post_probe = report.get("post_probe")
    if not isinstance(post_probe, dict):
        return None
    chain_height = post_probe.get("chain_height") or {}
    stateroot_height = post_probe.get("stateroot_height") or {}
    stateroot_root = post_probe.get("stateroot_root") or {}
    reference = post_probe.get("reference_stateroot") or {}
    proof: dict[str, Any] = {
        "status_after_post_probe": report.get("status"),
        "stateroot_matches_chain": post_probe.get("stateroot_matches_chain"),
    }
    if isinstance(chain_height, dict):
        proof["chain_height"] = chain_height.get("height")
    if isinstance(stateroot_height, dict):
        proof["stateroot_height"] = stateroot_height.get("height")
    if isinstance(stateroot_root, dict):
        proof["local_root"] = stateroot_root.get("root")
    if isinstance(reference, dict) and reference:
        proof["reference_matches_local"] = reference.get("matches_local")
        proof["successful_reference_samples"] = reference.get("successful_samples", 0)
        proof["reference_sample_count"] = reference.get("sample_count", 0)
    return proof


def build_sync_proof(report: dict) -> dict[str, Any]:
    samples = [sample for sample in report.get("height_samples") or [] if isinstance(sample, dict)]
    height_samples = [sample for sample in samples if "height" in sample]
    initial_height = height_samples[0].get("height") if height_samples else None
    final_height = report.get("last_height")
    try:
        advanced_blocks = max(int(final_height) - int(initial_height), 0)
    except (TypeError, ValueError):
        advanced_blocks = None
    proof: dict[str, Any] = {
        "sync_source": report.get("sync_source", "network"),
        "status": report.get("status"),
        "target_height": report.get("target_height"),
        "initial_height": initial_height,
        "final_height": final_height,
        "advanced_blocks": advanced_blocks,
        "elapsed_seconds": report.get("elapsed_seconds", 0.0),
        "average_blocks_per_second": report.get("blocks_per_second", 0.0),
        "height_sample_rate_summary": height_sample_rate_summary(report),
        "height_sample_count": len(height_samples),
        "height_sample_sources": ordered_sample_sources(samples),
        "metrics_sample_count": report.get("metrics_sample_count", 0),
        "transaction_work_summary": report.get("transaction_work_summary"),
        "sync_speed_floor_blocks_per_second": report.get("sync_speed_floor_blocks_per_second"),
        "sync_speed_ceiling_blocks_per_second": report.get("sync_speed_ceiling_blocks_per_second"),
        "sync_speed_shortfall_blocks_per_second": report.get(
            "sync_speed_shortfall_blocks_per_second", 0.0
        ),
        "sync_speed_overage_blocks_per_second": report.get(
            "sync_speed_overage_blocks_per_second", 0.0
        ),
        "sync_speed_band_met": report.get("sync_speed_band_met"),
    }
    if proof["sync_source"] == "fast-sync":
        proof["fast_sync_cache"] = latest_fast_sync_cache_proof(samples)
        fast_sync_report = report.get("fast_sync_report")
        if isinstance(fast_sync_report, dict):
            package = fast_sync_report.get("package")
            import_report = fast_sync_report.get("import")
            hot_metrics = fast_sync_report.get("hot_metrics")
            reference = fast_sync_report.get("reference")
            if isinstance(package, dict):
                proof["fast_sync_package"] = package
            if isinstance(import_report, dict):
                proof["fast_sync_import"] = import_report
            if isinstance(hot_metrics, dict):
                proof["fast_sync_hot_metrics"] = hot_metrics
            if isinstance(reference, dict):
                proof["fast_sync_reference"] = reference
    post_probe = sync_post_probe_proof(report)
    if post_probe is not None:
        proof["post_probe"] = post_probe
    return proof


def height_sample_rate_summary(report: dict) -> dict[str, Any]:
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

    sample_count = len([sample for sample in samples if isinstance(sample, dict)])
    if not intervals:
        return {
            "sample_count": sample_count,
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
        "sample_count": sample_count,
        "interval_count": len(intervals),
        "average_blocks_per_second": sum(rates) / len(rates),
        "min_blocks_per_second": min(rates),
        "max_blocks_per_second": max(rates),
        "slowest_interval": slowest,
        "fastest_interval": fastest,
    }


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
    target_ready_reader: Callable[[], bool] | None = None,
    node_output: Any | None = None,
    metrics_url: str | None = None,
    metrics_fetcher: Callable[[str], dict[str, float]] = fetch_prometheus_metrics,
    progress_reader: Callable[[], dict[str, Any]] | None = None,
    require_metrics_samples: bool = False,
    min_blocks_per_second: float | None = None,
    max_blocks_per_second: float | None = None,
    initial_height: int | None = None,
    sync_source: str = "network",
) -> dict:
    clock = clock or SystemClock()
    started_at = clock.time()
    samples: list[dict] = []
    last_height: int | None = None
    if initial_height is not None:
        last_height = int(initial_height)
        samples.append(
            {
                "elapsed_seconds": 0.0,
                "height": last_height,
                "height_source": "initial",
            }
        )
    elif height_reader is not None and sync_source in {"fast-sync", "import-chain"}:
        try:
            initial_probe_height = height_reader()
            if initial_probe_height is not None:
                last_height = int(initial_probe_height)
                samples.append(
                    {
                        "elapsed_seconds": 0.0,
                        "height": last_height,
                        "height_source": "fallback-initial",
                    }
                )
        except Exception as exc:  # pylint: disable=broad-except
            samples.append(
                {
                    "elapsed_seconds": 0.0,
                    "height_reader_error": str(exc),
                    "height_source": "fallback-initial",
                }
            )
    process = spawner(
        command,
        stdout=node_output if node_output is not None else subprocess.DEVNULL,
        stderr=subprocess.STDOUT,
        text=True,
    )
    status = "timeout"

    def read_height_from_fallback() -> tuple[int | None, str | None]:
        if height_reader is None:
            return None, None
        height = height_reader()
        if height is None:
            return None, None
        return int(height), "fallback"

    def confirm_rpc_height(sample: dict) -> bool:
        if height_reader is None:
            return True
        try:
            local_height, _source = read_height_from_fallback()
        except Exception as exc:  # pylint: disable=broad-except
            sample["height_reader_error"] = str(exc)
            return False
        if local_height is None:
            return False
        sample["stale_rpc_height"] = sample.get("height")
        sample["height"] = local_height
        sample["height_source"] = "fallback-confirmation"
        return local_height >= target_height

    def attach_metrics(sample: dict) -> None:
        if not metrics_url:
            return
        try:
            sample["metrics"] = metrics_fetcher(metrics_url)
        except Exception as exc:  # pylint: disable=broad-except
            sample["metrics_error"] = str(exc)

    def attach_progress(sample: dict) -> None:
        if progress_reader is None:
            return
        try:
            progress = progress_reader()
            if isinstance(progress, dict):
                sample.update(progress)
        except Exception as exc:  # pylint: disable=broad-except
            sample["progress_error"] = str(exc)

    def append_height_sample(height: int, source: str) -> None:
        sample = {
            "elapsed_seconds": round(clock.time() - started_at, 3),
            "height": height,
            "height_source": source,
        }
        attach_progress(sample)
        attach_metrics(sample)
        samples.append(sample)

    def target_is_ready() -> bool:
        if target_ready_reader is None:
            return True
        try:
            return bool(target_ready_reader())
        except Exception:  # pylint: disable=broad-except
            return False

    try:
        while clock.time() - started_at < max_seconds:
            if process.poll() is not None:
                if process.returncode == 0 and (
                    last_height is None or last_height < target_height
                ):
                    try:
                        block_count = int(rpc(rpc_url, "getblockcount", [], 5.0))
                        last_height = block_count - 1
                        append_height_sample(last_height, "rpc-final")
                    except Exception:  # pylint: disable=broad-except
                        try:
                            height, source = read_height_from_fallback()
                            if height is not None:
                                last_height = height
                                append_height_sample(height, source or "fallback")
                        except Exception:  # pylint: disable=broad-except
                            pass
                status = classify_process_exit(
                    returncode=process.returncode,
                    last_height=last_height,
                    target_height=target_height,
                    target_ready=target_is_ready(),
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
                attach_progress(sample)
                attach_metrics(sample)
                samples.append(sample)
                print(json.dumps(sample), flush=True)
                target_confirmed = height >= target_height and confirm_rpc_height(sample)
                if sample.get("height_source") == "fallback-confirmation":
                    last_height = int(sample["height"])
                if target_confirmed and target_is_ready():
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
                attach_progress(sample)
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
                if (
                    last_height is not None
                    and last_height >= target_height
                    and target_is_ready()
                ):
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
                append_height_sample(height, "fallback-final")
                if (
                    process.returncode == 0
                    and last_height >= target_height
                    and target_is_ready()
                ):
                    status = "target-reached"
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

    metrics_samples = metrics_sample_count(samples)
    metrics_summary = summarize_metric_samples(samples)
    tx_work_summary = transaction_work_summary(metrics_summary)
    rate_summary = height_sample_rate_summary({"height_samples": samples})
    if require_metrics_samples and metrics_url and status == "target-reached" and metrics_samples == 0:
        status = "metrics-unavailable"
    sync_speed_shortfall = 0.0
    sync_speed_overage = 0.0
    speed_floor_value = (
        float(rate_summary["min_blocks_per_second"])
        if rate_summary["interval_count"] > 0
        else 0.0
    )
    speed_ceiling_value = (
        float(rate_summary["max_blocks_per_second"])
        if rate_summary["interval_count"] > 0
        else 0.0
    )
    sync_speed_floor_met = (
        min_blocks_per_second is None or speed_floor_value >= min_blocks_per_second
    )
    sync_speed_ceiling_met = (
        max_blocks_per_second is None
        or (rate_summary["interval_count"] > 0 and speed_ceiling_value <= max_blocks_per_second)
    )
    if min_blocks_per_second is not None and not sync_speed_floor_met:
        sync_speed_shortfall = min_blocks_per_second - speed_floor_value
        if status == "target-reached":
            status = "sync-speed-too-slow"
    if max_blocks_per_second is not None and not sync_speed_ceiling_met:
        sync_speed_overage = max(speed_ceiling_value - max_blocks_per_second, 0.0)
        if status == "target-reached":
            status = "sync-speed-too-fast"
    if (
        min_blocks_per_second is not None
        and status == "target-reached"
        and not tx_work_summary["observed_transaction_work"]
    ):
        status = "transaction-work-unproven"

    report = {
        "command": command,
        "sync_source": sync_source,
        "pid": int(process.pid),
        "status": status,
        "target_height": target_height,
        "last_height": last_height,
        "elapsed_seconds": round(elapsed, 3),
        "blocks_per_second": blocks_per_second,
        "height_samples": samples,
        "height_sample_rate_summary": rate_summary,
        "metrics_sample_count": metrics_samples,
        "metrics_summary": metrics_summary,
        "transaction_work_summary": tx_work_summary,
        "sync_speed_floor_blocks_per_second": min_blocks_per_second,
        "sync_speed_ceiling_blocks_per_second": max_blocks_per_second,
        "sync_speed_shortfall_blocks_per_second": sync_speed_shortfall,
        "sync_speed_overage_blocks_per_second": sync_speed_overage,
        "sync_speed_band_met": sync_speed_floor_met and sync_speed_ceiling_met,
        "returncode": process.returncode,
    }
    report["sync_proof"] = build_sync_proof(report)
    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run neo-node until a target block height and emit a JSON replay summary."
    )
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--node-bin", default=Path("target/release/neo-node"), type=Path)
    parser.add_argument(
        "--import-chain",
        default=None,
        type=Path,
        help="Optional chain.acc file to import before live sync starts.",
    )
    parser.add_argument(
        "--fast-sync",
        action="store_true",
        help="Run neo-node's built-in fast-sync package import before live sync starts.",
    )
    parser.add_argument(
        "--fast-sync-cache",
        default=None,
        type=Path,
        help="Optional cache directory for the built-in fast-sync package.",
    )
    parser.add_argument("--rpc", default="http://127.0.0.1:21332")
    parser.add_argument("--target-height", required=True, type=int)
    parser.add_argument("--poll-interval", default=None, type=float)
    parser.add_argument("--max-seconds", default=900.0, type=float)
    parser.add_argument("--db", default=None, type=Path)
    parser.add_argument("--probe-bin", default=Path("target/release/neo-db-probe"), type=Path)
    parser.add_argument(
        "--storage-provider",
        default=DEFAULT_STORAGE_PROVIDER,
        choices=["mdbx", "rocksdb"],
        help="Storage backend used by neo-db-probe for post-run proof reads.",
    )
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
    parser.add_argument(
        "--require-metrics-samples",
        action="store_true",
        help="Fail a target-reached run unless at least one metrics poll returns usable metrics.",
    )
    parser.add_argument(
        "--sync-speed-floor-bps",
        default=DEFAULT_SYNC_SPEED_FLOOR_BPS,
        type=float,
        help=(
            "Fail a target-reached run when measured replay speed is below this "
            "floor. The production proof target requires at least 1500 blocks/s."
        ),
    )
    parser.add_argument(
        "--sync-speed-ceiling-bps",
        default=None,
        type=float,
        help="Fail a target-reached run when measured replay speed is above this ceiling.",
    )
    parser.add_argument(
        "--initial-height",
        default=None,
        type=int,
        help="Optional starting height used for replay/import BPS calculation.",
    )
    args = parser.parse_args()
    if args.import_chain is not None and args.fast_sync:
        parser.error("--import-chain and --fast-sync are mutually exclusive")
    if args.fast_sync_cache is not None and not args.fast_sync:
        parser.error("--fast-sync-cache requires --fast-sync")
    if (
        args.sync_speed_floor_bps is not None
        and args.sync_speed_floor_bps < DEFAULT_SYNC_SPEED_FLOOR_BPS
    ):
        parser.error(
            "--sync-speed-floor-bps must be >= "
            f"{DEFAULT_SYNC_SPEED_FLOOR_BPS:g} for production proof"
        )
    if (args.fast_sync or args.import_chain is not None) and args.stateroot_db is not None:
        args.require_stateroot_height_match = True
    if args.poll_interval is None:
        args.poll_interval = (
            DEFAULT_IMPORT_POLL_INTERVAL_SECONDS
            if args.fast_sync or args.import_chain is not None
            else DEFAULT_POLL_INTERVAL_SECONDS
        )
    return args


def sync_source_for_args(args: argparse.Namespace) -> str:
    if args.fast_sync:
        return "fast-sync"
    if args.import_chain is not None:
        return "import-chain"
    return "network"


def fast_sync_cache_dir_for_args(args: argparse.Namespace) -> Path | None:
    if not args.fast_sync:
        return None
    if args.fast_sync_cache is not None:
        return args.fast_sync_cache
    if args.db is not None:
        return args.db / "fast-sync"
    return Path("data") / "fast-sync"


def fast_sync_report_path_for_args(args: argparse.Namespace) -> Path | None:
    if not args.fast_sync:
        return None
    if args.db is not None:
        return args.db / "fast-sync" / "fast-sync-report.json"
    if args.fast_sync_cache is not None:
        return args.fast_sync_cache / "fast-sync-report.json"
    return Path("data") / "fast-sync" / "fast-sync-report.json"


def main() -> int:
    args = parse_args()
    height_reader = None
    if args.db is not None:
        height_reader = lambda: read_probe_ledger_height(
            args.db,
            args.probe_bin,
            args.storage_provider,
        )
    target_ready_reader = None
    if args.require_stateroot_height_match and args.stateroot_db is not None:
        target_ready_reader = (
            lambda: stateroot_covers_observed_chain_height(
                target_height=args.target_height,
                observed_chain_height=height_reader() if height_reader is not None else None,
                stateroot_height=read_probe_mpt_state_height(
                    args.stateroot_db,
                    args.probe_bin,
                    args.storage_provider,
                ),
            )
        )
    fast_sync_cache_dir = fast_sync_cache_dir_for_args(args)
    fast_sync_report_path = fast_sync_report_path_for_args(args)
    progress_reader = (
        (lambda: fast_sync_cache_progress(fast_sync_cache_dir))
        if fast_sync_cache_dir is not None
        else None
    )
    if args.node_output_log is not None:
        args.node_output_log.parent.mkdir(parents=True, exist_ok=True)
    with (
        args.node_output_log.open("a", encoding="utf-8")
        if args.node_output_log is not None
        else contextlib.nullcontext(None)
    ) as node_output:
        report = run_until_target(
            command=node_command(
                args.node_bin,
                args.config,
                args.target_height,
                import_chain=args.import_chain,
                fast_sync=args.fast_sync,
                fast_sync_cache=args.fast_sync_cache,
                fast_sync_report=fast_sync_report_path,
            ),
            rpc_url=args.rpc,
            target_height=args.target_height,
            poll_interval=args.poll_interval,
            max_seconds=args.max_seconds,
            height_reader=height_reader,
            node_output=node_output,
            metrics_url=args.metrics_url,
            require_metrics_samples=args.require_metrics_samples,
            min_blocks_per_second=args.sync_speed_floor_bps,
            max_blocks_per_second=args.sync_speed_ceiling_bps,
            initial_height=args.initial_height,
            target_ready_reader=target_ready_reader,
            sync_source=sync_source_for_args(args),
            progress_reader=progress_reader,
        )
    report = attach_fast_sync_report(report, fast_sync_report_path)
    report = attach_post_probe_report(
        report,
        chain_db=args.db,
        stateroot_db=args.stateroot_db,
        probe_bin=args.probe_bin,
        storage_provider=args.storage_provider,
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
