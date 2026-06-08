#!/usr/bin/env python3
"""
Continuously validate neo-rs state roots against official Neo RPC nodes.

This validator compares the local neo-rs `getstateroot` output with one or more
reference RPC endpoints and keeps running as the local node syncs. It persists a
checkpoint only after each block has been fully compared, so transient RPC
failures never cause silent skips.

Examples:
    python3 scripts/continuous-stateroot-validation.py

    python3 scripts/continuous-stateroot-validation.py \
        --local-config neo_mainnet_node.toml \
        --once

    python3 scripts/continuous-stateroot-validation.py \
        --local http://127.0.0.1:20332 \
        --reference http://seed1.neo.org:10332,http://seed2.neo.org:10332
"""

from __future__ import annotations

import argparse
import base64
import gzip
import http.client
import json
import os
import sys
import time
import tomllib
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from tempfile import NamedTemporaryFile
from typing import Callable
from urllib.parse import ParseResult, urlparse


DEFAULT_LOCAL_RPC = "http://127.0.0.1:10332"
DEFAULT_REFERENCE_RPCS = [
    "http://seed1.neo.org:10332",
    "http://seed2.neo.org:10332",
    "http://seed3.neo.org:10332",
    "http://seed4.neo.org:10332",
    "http://seed5.neo.org:10332",
]
DEFAULT_STATUS_FILE = "/tmp/stateroot-validation.json"
DEFAULT_RESUME_FILE = "/tmp/stateroot-last-validated"


@dataclass(frozen=True)
class RpcEndpoint:
    name: str
    url: str
    username: str | None = None
    password: str | None = None

    @property
    def display_name(self) -> str:
        return self.name or self.url


@dataclass
class RootSample:
    index: int
    root: str | None
    endpoint: str | None = None
    error: str | None = None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Continuously validate neo-rs state roots against official Neo RPC nodes"
    )
    parser.add_argument(
        "--local",
        default=None,
        help=f"Local neo-rs RPC URL (default: {DEFAULT_LOCAL_RPC} or --local-config)",
    )
    parser.add_argument(
        "--local-config",
        default=None,
        help="Optional neo-node TOML config used to infer local RPC bind/port/auth",
    )
    parser.add_argument("--local-user", default=None, help="Local RPC basic auth username")
    parser.add_argument("--local-pass", default=None, help="Local RPC basic auth password")
    parser.add_argument(
        "--reference",
        action="append",
        default=[],
        help=(
            "Reference RPC URL(s). Repeat the flag or pass comma-separated values. "
            "Defaults to official Neo mainnet seeds."
        ),
    )
    parser.add_argument(
        "--reference-user",
        default=None,
        help="Optional basic auth username applied to all reference RPC URLs",
    )
    parser.add_argument(
        "--reference-pass",
        default=None,
        help="Optional basic auth password applied to all reference RPC URLs",
    )
    parser.add_argument(
        "--start",
        type=int,
        default=None,
        help="Start block index. Overrides any resume file.",
    )
    parser.add_argument(
        "--stop-at",
        type=int,
        default=None,
        help="Stop after validating this block index.",
    )
    parser.add_argument(
        "--batch",
        type=int,
        default=500,
        help="Maximum number of blocks fetched per comparison batch (default: 500)",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=8,
        help="Parallel workers per local/reference fetch batch (default: 8)",
    )
    parser.add_argument(
        "--status-file",
        "--output",
        dest="status_file",
        default=DEFAULT_STATUS_FILE,
        help=f"Write structured status JSON to this file (default: {DEFAULT_STATUS_FILE})",
    )
    parser.add_argument(
        "--resume-file",
        default=DEFAULT_RESUME_FILE,
        help=f"Checkpoint file storing the last fully validated block (default: {DEFAULT_RESUME_FILE})",
    )
    parser.add_argument(
        "--poll-interval",
        type=float,
        default=5.0,
        help="Seconds between sync polls while waiting (default: 5)",
    )
    parser.add_argument(
        "--rpc-timeout",
        type=float,
        default=15.0,
        help="RPC timeout in seconds for local/reference calls (default: 15)",
    )
    parser.add_argument(
        "--retry-rounds",
        type=int,
        default=2,
        help="How many rounds to try across reference endpoints before pausing (default: 2)",
    )
    parser.add_argument(
        "--retry-backoff",
        type=float,
        default=0.75,
        help="Backoff in seconds between reference retry rounds (default: 0.75)",
    )
    parser.add_argument(
        "--mismatch-limit",
        type=int,
        default=10,
        help="Abort once this many mismatches have been observed (default: 10)",
    )
    parser.add_argument(
        "--recent-error-limit",
        type=int,
        default=50,
        help="Keep only this many recent errors in the status file (default: 50)",
    )
    parser.add_argument(
        "--recent-mismatch-limit",
        type=int,
        default=50,
        help="Keep only this many recent mismatches in the status file (default: 50)",
    )
    parser.add_argument(
        "--once",
        action="store_true",
        help="Validate only up to the local state-root height seen at startup, then exit",
    )
    args = parser.parse_args()

    if args.batch < 1:
        parser.error("--batch must be >= 1")
    if args.workers < 1:
        parser.error("--workers must be >= 1")
    if args.retry_rounds < 1:
        parser.error("--retry-rounds must be >= 1")
    if args.mismatch_limit < 1:
        parser.error("--mismatch-limit must be >= 1")
    if args.stop_at is not None and args.stop_at < 0:
        parser.error("--stop-at must be >= 0")
    if args.start is not None and args.start < 0:
        parser.error("--start must be >= 0")

    return args


def timestamp() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")


def short_time() -> str:
    return datetime.now().strftime("%H:%M:%S")


def format_host_for_url(host: str) -> str:
    if ":" in host and not host.startswith("["):
        return f"[{host}]"
    return host


def pick_local_connect_host(bind_address: str | None) -> str:
    bind = (bind_address or "127.0.0.1").strip()
    if bind in {"", "0.0.0.0", "::", "[::]"}:
        return "127.0.0.1"
    return bind


def normalize_rpc_url(raw_url: str) -> str:
    candidate = raw_url.strip()
    if not candidate:
        raise ValueError("RPC URL cannot be empty")
    if "://" not in candidate:
        candidate = f"http://{candidate}"
    parsed = urlparse(candidate)
    if not parsed.scheme or not parsed.hostname:
        raise ValueError(f"Invalid RPC URL: {raw_url}")
    path = parsed.path or "/"
    normalized = ParseResult(
        scheme=parsed.scheme,
        netloc=parsed.netloc,
        path=path,
        params=parsed.params,
        query=parsed.query,
        fragment=parsed.fragment,
    )
    return normalized.geturl()


def flatten_reference_args(values: list[str]) -> list[str]:
    urls: list[str] = []
    for value in values:
        for item in value.split(","):
            item = item.strip()
            if item:
                urls.append(item)
    return urls


def load_local_rpc_settings(config_path: str) -> dict[str, str | None]:
    with open(config_path, "rb") as handle:
        config = tomllib.load(handle)

    rpc = config.get("rpc")
    if not isinstance(rpc, dict):
        raise ValueError(f"{config_path} does not contain an [rpc] section")
    if rpc.get("enabled") is False:
        raise ValueError(f"{config_path} has rpc.enabled = false")

    host = pick_local_connect_host(str(rpc.get("bind_address") or "127.0.0.1"))
    port = int(rpc.get("port") or 10332)
    auth_enabled = bool(rpc.get("auth_enabled", False))
    username = rpc.get("rpc_user")
    password = rpc.get("rpc_pass")

    return {
        "url": f"http://{format_host_for_url(host)}:{port}",
        "username": str(username) if auth_enabled and username is not None else None,
        "password": str(password) if auth_enabled and password is not None else None,
    }


def resolve_local_endpoint(args: argparse.Namespace) -> RpcEndpoint:
    config_values: dict[str, str | None] = {}
    if args.local_config:
        config_values = load_local_rpc_settings(args.local_config)

    url = args.local or config_values.get("url") or DEFAULT_LOCAL_RPC
    username = (
        args.local_user
        or os.getenv("NEO_RPC_USER")
        or config_values.get("username")
    )
    password = (
        args.local_pass
        or os.getenv("NEO_RPC_PASS")
        or config_values.get("password")
    )

    return RpcEndpoint(
        name="local",
        url=normalize_rpc_url(url),
        username=username,
        password=password,
    )


def resolve_reference_endpoints(args: argparse.Namespace) -> list[RpcEndpoint]:
    urls = flatten_reference_args(args.reference) or list(DEFAULT_REFERENCE_RPCS)
    username = args.reference_user or os.getenv("REFERENCE_RPC_USER")
    password = args.reference_pass or os.getenv("REFERENCE_RPC_PASS")
    endpoints: list[RpcEndpoint] = []
    for index, url in enumerate(urls, start=1):
        endpoints.append(
            RpcEndpoint(
                name=f"reference[{index}]",
                url=normalize_rpc_url(url),
                username=username,
                password=password,
            )
        )
    return endpoints


def atomic_write(path: str | None, payload: str) -> None:
    if not path:
        return
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp_path: str | None = None
    try:
        with NamedTemporaryFile(
            "w",
            dir=target.parent,
            encoding="utf-8",
            delete=False,
        ) as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
            tmp_path = handle.name
        os.replace(tmp_path, target)
    finally:
        if tmp_path and os.path.exists(tmp_path):
            os.unlink(tmp_path)


def save_json(path: str | None, payload: dict) -> None:
    atomic_write(path, json.dumps(payload, indent=2, sort_keys=True) + "\n")


def load_resume(path: str | None) -> int | None:
    if not path:
        return None
    target = Path(path)
    if not target.exists():
        return None
    raw = target.read_text(encoding="utf-8").strip()
    if not raw:
        return None
    return int(raw)


def save_resume(path: str | None, last_validated_block: int) -> None:
    atomic_write(path, f"{last_validated_block}\n")


def add_recent(items: list[dict], entry: dict, limit: int) -> None:
    items.append(entry)
    if len(items) > limit:
        del items[: len(items) - limit]


def rpc_call(
    endpoint: RpcEndpoint,
    method: str,
    params: list | None = None,
    timeout: float = 15.0,
) -> tuple[object | None, str | None]:
    payload = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params or [],
        "id": 1,
    }
    parsed = urlparse(endpoint.url)
    port = parsed.port or (443 if parsed.scheme == "https" else 80)
    path = parsed.path or "/"
    if parsed.query:
        path = f"{path}?{parsed.query}"
    headers = {
        "Content-Type": "application/json",
        "Accept-Encoding": "gzip",
    }

    if endpoint.username is not None and endpoint.password is not None:
        token = base64.b64encode(
            f"{endpoint.username}:{endpoint.password}".encode("utf-8")
        ).decode("ascii")
        headers["Authorization"] = f"Basic {token}"

    connection_cls: type[http.client.HTTPConnection]
    if parsed.scheme == "https":
        connection_cls = http.client.HTTPSConnection
    else:
        connection_cls = http.client.HTTPConnection

    try:
        conn = connection_cls(parsed.hostname, port, timeout=timeout)
        conn.request("POST", path, json.dumps(payload), headers)
        response = conn.getresponse()
        raw = response.read()
        status = response.status
        reason = response.reason
        content_encoding = response.getheader("Content-Encoding", "")
        conn.close()

        if content_encoding.lower() == "gzip" or raw[:2] == b"\x1f\x8b":
            raw = gzip.decompress(raw)

        body = raw.decode("utf-8") if raw else ""
        if status >= 400:
            return None, f"HTTP {status} {reason}: {body[:200]}"

        result = json.loads(body)
        if "error" in result and result["error"] is not None:
            return None, json.dumps(result["error"], sort_keys=True)
        return result.get("result"), None
    except Exception as exc:  # pylint: disable=broad-except
        return None, str(exc)


def get_state_root(endpoint: RpcEndpoint, index: int, timeout: float) -> RootSample:
    result, error = rpc_call(endpoint, "getstateroot", [index], timeout)
    if error:
        return RootSample(index=index, root=None, endpoint=endpoint.display_name, error=error)
    if isinstance(result, dict) and "roothash" in result:
        return RootSample(
            index=index,
            root=result["roothash"],
            endpoint=endpoint.display_name,
        )
    return RootSample(
        index=index,
        root=None,
        endpoint=endpoint.display_name,
        error=f"unexpected response: {result}",
    )


def get_state_height(
    endpoint: RpcEndpoint, timeout: float
) -> tuple[int | None, int | None, str | None]:
    result, error = rpc_call(endpoint, "getstateheight", [], timeout)
    if error:
        return None, None, error
    if not isinstance(result, dict):
        return None, None, f"unexpected response: {result}"
    return (
        result.get("localrootindex"),
        result.get("validatedrootindex"),
        None,
    )


def get_block_count(endpoint: RpcEndpoint, timeout: float) -> tuple[int | None, str | None]:
    result, error = rpc_call(endpoint, "getblockcount", [], timeout)
    if error:
        return None, error
    if isinstance(result, int):
        return result, None
    return None, f"unexpected response: {result}"


def fetch_batch(
    start: int,
    end: int,
    workers: int,
    fetch_one: Callable[[int], RootSample],
) -> dict[int, RootSample]:
    results: dict[int, RootSample] = {}
    with ThreadPoolExecutor(max_workers=max(1, workers)) as executor:
        future_map = {
            executor.submit(fetch_one, index): index for index in range(start, end + 1)
        }
        for future in as_completed(future_map):
            index = future_map[future]
            results[index] = future.result()
    return results


def fetch_reference_root(
    endpoints: list[RpcEndpoint],
    index: int,
    timeout: float,
    retry_rounds: int,
    retry_backoff: float,
) -> RootSample:
    if not endpoints:
        return RootSample(index=index, root=None, error="no reference endpoints configured")

    errors: list[str] = []
    start_offset = index % len(endpoints)
    ordered = endpoints[start_offset:] + endpoints[:start_offset]

    for round_index in range(retry_rounds):
        for endpoint in ordered:
            sample = get_state_root(endpoint, index, timeout)
            if sample.root is not None:
                return sample
            errors.append(f"{endpoint.display_name}: {sample.error}")
        if round_index + 1 < retry_rounds:
            time.sleep(retry_backoff * (round_index + 1))

    return RootSample(
        index=index,
        root=None,
        error=" | ".join(errors[-len(ordered) * retry_rounds :]),
    )


def build_status_payload(
    *,
    local_endpoint: RpcEndpoint,
    reference_endpoints: list[RpcEndpoint],
    start_block: int,
    next_block: int,
    last_validated_block: int,
    total_compared: int,
    total_matched: int,
    total_mismatched: int,
    total_errors: int,
    local_state_height: int | None,
    local_validated_height: int | None,
    local_block_count: int | None,
    mismatches: list[dict],
    errors: list[dict],
    started_at: float,
    status: str,
    target_stop_at: int | None,
) -> dict:
    elapsed = max(time.time() - started_at, 0.0)
    rate = total_compared / elapsed if elapsed > 0 else 0.0
    return {
        "timestamp": timestamp(),
        "status": status,
        "local_url": local_endpoint.url,
        "reference_urls": [endpoint.url for endpoint in reference_endpoints],
        "start_block": start_block,
        "target_stop_at": target_stop_at,
        "next_block": next_block,
        "last_validated_block": last_validated_block,
        "local_state_height": local_state_height,
        "local_validated_height": local_validated_height,
        "local_block_count": local_block_count,
        "total_compared": total_compared,
        "total_matched": total_matched,
        "total_mismatched": total_mismatched,
        "total_errors": total_errors,
        "match_percentage": (total_matched / total_compared * 100.0)
        if total_compared
        else 0.0,
        "rate_per_second": rate,
        "elapsed_seconds": elapsed,
        "recent_mismatches": mismatches,
        "recent_errors": errors,
    }


def write_status(
    args: argparse.Namespace,
    local_endpoint: RpcEndpoint,
    reference_endpoints: list[RpcEndpoint],
    start_block: int,
    next_block: int,
    last_validated_block: int,
    total_compared: int,
    total_matched: int,
    total_mismatched: int,
    total_errors: int,
    local_state_height: int | None,
    local_validated_height: int | None,
    local_block_count: int | None,
    mismatches: list[dict],
    errors: list[dict],
    started_at: float,
    status: str,
    target_stop_at: int | None,
) -> None:
    save_json(
        args.status_file,
        build_status_payload(
            local_endpoint=local_endpoint,
            reference_endpoints=reference_endpoints,
            start_block=start_block,
            next_block=next_block,
            last_validated_block=last_validated_block,
            total_compared=total_compared,
            total_matched=total_matched,
            total_mismatched=total_mismatched,
            total_errors=total_errors,
            local_state_height=local_state_height,
            local_validated_height=local_validated_height,
            local_block_count=local_block_count,
            mismatches=mismatches,
            errors=errors,
            started_at=started_at,
            status=status,
            target_stop_at=target_stop_at,
        ),
    )


def main() -> int:
    args = parse_args()
    try:
        local_endpoint = resolve_local_endpoint(args)
        reference_endpoints = resolve_reference_endpoints(args)
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    resumed_block = load_resume(args.resume_file)
    next_block = (
        args.start
        if args.start is not None
        else (resumed_block + 1 if resumed_block is not None else 0)
    )
    start_block = next_block
    last_validated_block = next_block - 1

    print("=== Neo-RS Continuous State Root Validator ===")
    print(f"Local:           {local_endpoint.url}")
    print(f"References:      {', '.join(endpoint.url for endpoint in reference_endpoints)}")
    print(f"Starting block:  {next_block}")
    if args.local_config:
        print(f"Local config:    {args.local_config}")
    print(f"Batch size:      {args.batch}")
    print(f"Workers:         {args.workers}")
    print(f"Resume file:     {args.resume_file}")
    print(f"Status file:     {args.status_file}")
    print()

    live_reference_block = None
    for endpoint in reference_endpoints:
        live_reference_block, probe_error = get_block_count(endpoint, args.rpc_timeout)
        if probe_error is None:
            print(
                f"Reference probe: {endpoint.display_name} reachable at block {live_reference_block}"
            )
            break
    else:
        print("Reference probe: no reference endpoint responded yet; will retry during comparison")

    total_compared = 0
    total_matched = 0
    total_mismatched = 0
    total_errors = 0
    recent_mismatches: list[dict] = []
    recent_errors: list[dict] = []
    target_stop_at = args.stop_at
    started_at = time.time()
    one_shot_locked = False

    try:
        while True:
            local_state_height, local_validated_height, local_error = get_state_height(
                local_endpoint, args.rpc_timeout
            )
            local_block_count, _ = get_block_count(local_endpoint, args.rpc_timeout)

            if local_error:
                print(
                    f"\r[{short_time()}] Waiting for local node RPC at {local_endpoint.url}: "
                    f"{local_error}",
                    end="",
                    flush=True,
                )
                write_status(
                    args,
                    local_endpoint,
                    reference_endpoints,
                    start_block,
                    next_block,
                    last_validated_block,
                    total_compared,
                    total_matched,
                    total_mismatched,
                    total_errors,
                    local_state_height,
                    local_validated_height,
                    local_block_count,
                    recent_mismatches,
                    recent_errors,
                    started_at,
                    "WAITING_LOCAL_RPC",
                    target_stop_at,
                )
                time.sleep(args.poll_interval)
                continue

            if local_state_height is None:
                local_state_height = -1

            if args.once and not one_shot_locked:
                target_stop_at = (
                    min(local_state_height, target_stop_at)
                    if target_stop_at is not None
                    else local_state_height
                )
                one_shot_locked = True
                print(
                    f"[{short_time()}] One-shot target locked at block {target_stop_at}",
                    flush=True,
                )

            if target_stop_at is not None and next_block > target_stop_at:
                print(
                    f"[{short_time()}] Validation complete at block {last_validated_block}",
                    flush=True,
                )
                write_status(
                    args,
                    local_endpoint,
                    reference_endpoints,
                    start_block,
                    next_block,
                    last_validated_block,
                    total_compared,
                    total_matched,
                    total_mismatched,
                    total_errors,
                    local_state_height,
                    local_validated_height,
                    local_block_count,
                    recent_mismatches,
                    recent_errors,
                    started_at,
                    "COMPLETE" if total_mismatched == 0 else "COMPLETE_WITH_MISMATCHES",
                    target_stop_at,
                )
                break

            compare_end = min(local_state_height, next_block + args.batch - 1)
            if target_stop_at is not None:
                compare_end = min(compare_end, target_stop_at)

            if compare_end < next_block:
                elapsed = max(time.time() - started_at, 0.0)
                rate = total_compared / elapsed if elapsed > 0 else 0.0
                print(
                    f"\r[{short_time()}] Local block={local_block_count} state_root={local_state_height} "
                    f"validated={last_validated_block} rate={rate:.1f}/s mismatches={total_mismatched} "
                    f"waiting...",
                    end="",
                    flush=True,
                )
                write_status(
                    args,
                    local_endpoint,
                    reference_endpoints,
                    start_block,
                    next_block,
                    last_validated_block,
                    total_compared,
                    total_matched,
                    total_mismatched,
                    total_errors,
                    local_state_height,
                    local_validated_height,
                    local_block_count,
                    recent_mismatches,
                    recent_errors,
                    started_at,
                    "WAITING_FOR_SYNC",
                    target_stop_at,
                )
                time.sleep(args.poll_interval)
                continue

            print(
                f"\n[{short_time()}] Comparing blocks {next_block}-{compare_end} "
                f"(local state root {local_state_height}, local block {local_block_count})",
                flush=True,
            )

            local_roots = fetch_batch(
                next_block,
                compare_end,
                args.workers,
                lambda index: get_state_root(local_endpoint, index, args.rpc_timeout),
            )
            reference_roots = fetch_batch(
                next_block,
                compare_end,
                args.workers,
                lambda index: fetch_reference_root(
                    reference_endpoints,
                    index,
                    args.rpc_timeout,
                    args.retry_rounds,
                    args.retry_backoff,
                ),
            )

            blocked_index: int | None = None
            batch_matches = 0
            batch_mismatches = 0

            for index in range(next_block, compare_end + 1):
                local_sample = local_roots[index]
                reference_sample = reference_roots[index]

                if local_sample.root is None:
                    total_errors += 1
                    blocked_index = index
                    add_recent(
                        recent_errors,
                        {
                            "timestamp": timestamp(),
                            "index": index,
                            "side": "local",
                            "endpoint": local_sample.endpoint,
                            "error": local_sample.error,
                        },
                        args.recent_error_limit,
                    )
                    print(
                        f"  paused at block {index}: local RPC error via "
                        f"{local_sample.endpoint}: {local_sample.error}",
                        flush=True,
                    )
                    break

                if reference_sample.root is None:
                    total_errors += 1
                    blocked_index = index
                    add_recent(
                        recent_errors,
                        {
                            "timestamp": timestamp(),
                            "index": index,
                            "side": "reference",
                            "endpoint": reference_sample.endpoint,
                            "error": reference_sample.error,
                        },
                        args.recent_error_limit,
                    )
                    print(
                        f"  paused at block {index}: reference RPC error: "
                        f"{reference_sample.error}",
                        flush=True,
                    )
                    break

                total_compared += 1

                if local_sample.root == reference_sample.root:
                    total_matched += 1
                    batch_matches += 1
                else:
                    total_mismatched += 1
                    batch_mismatches += 1
                    add_recent(
                        recent_mismatches,
                        {
                            "timestamp": timestamp(),
                            "index": index,
                            "local": local_sample.root,
                            "reference": reference_sample.root,
                            "reference_endpoint": reference_sample.endpoint,
                        },
                        args.recent_mismatch_limit,
                    )
                    print(
                        f"  MISMATCH block {index}: local={local_sample.root} "
                        f"reference={reference_sample.root} via {reference_sample.endpoint}",
                        flush=True,
                    )

                last_validated_block = index

                if total_mismatched >= args.mismatch_limit:
                    save_resume(args.resume_file, last_validated_block)
                    write_status(
                        args,
                        local_endpoint,
                        reference_endpoints,
                        start_block,
                        last_validated_block + 1,
                        last_validated_block,
                        total_compared,
                        total_matched,
                        total_mismatched,
                        total_errors,
                        local_state_height,
                        local_validated_height,
                        local_block_count,
                        recent_mismatches,
                        recent_errors,
                        started_at,
                        "FAIL",
                        target_stop_at,
                    )
                    print(
                        f"  aborting after {total_mismatched} mismatches "
                        f"(limit {args.mismatch_limit})",
                        flush=True,
                    )
                    return 1

            if last_validated_block >= 0:
                save_resume(args.resume_file, last_validated_block)

            next_block = last_validated_block + 1

            elapsed = max(time.time() - started_at, 0.0)
            rate = total_compared / elapsed if elapsed > 0 else 0.0
            status = "PASS" if total_mismatched == 0 else "FAIL"

            print(
                f"  batch summary: {batch_matches} match, {batch_mismatches} mismatch | "
                f"total={total_compared} rate={rate:.1f}/s next={next_block}",
                flush=True,
            )

            write_status(
                args,
                local_endpoint,
                reference_endpoints,
                start_block,
                next_block,
                last_validated_block,
                total_compared,
                total_matched,
                total_mismatched,
                total_errors,
                local_state_height,
                local_validated_height,
                local_block_count,
                recent_mismatches,
                recent_errors,
                started_at,
                status if blocked_index is None else "PAUSED_ON_RPC_ERROR",
                target_stop_at,
            )

            if blocked_index is not None:
                time.sleep(args.poll_interval)

    except KeyboardInterrupt:
        print("\nInterrupted by user", file=sys.stderr)
        write_status(
            args,
            local_endpoint,
            reference_endpoints,
            start_block,
            next_block,
            last_validated_block,
            total_compared,
            total_matched,
            total_mismatched,
            total_errors,
            None,
            None,
            None,
            recent_mismatches,
            recent_errors,
            started_at,
            "INTERRUPTED",
            target_stop_at,
        )
        return 130

    return 0


if __name__ == "__main__":
    sys.exit(main())
