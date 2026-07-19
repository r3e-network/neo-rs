#!/usr/bin/env python3
"""
Binary-search for the first block where neo-rs and the C# reference node
disagree on the state root.

Usage:
    # Start the local node first, then:
    python3 scripts/find-stateroot-divergence.py

    # Or with custom endpoints:
    python3 scripts/find-stateroot-divergence.py \
        --local http://127.0.0.1:20332 \
        --reference http://seed1.neo.org:10332 \
        --max-height 295097
"""
from __future__ import annotations

import argparse
import gzip
import http.client
import json
import os
import subprocess
import sys
import time
from urllib.parse import urlparse


LOCAL_RPC = "http://127.0.0.1:20332"
REFERENCE_RPC = "http://seed1.neo.org:10332"
FALLBACK_SEEDS = [
    "http://seed2.neo.org:10332",
    "http://seed3.neo.org:10332",
    "http://seed4.neo.org:10332",
    "http://seed5.neo.org:10332",
]
MAX_HEIGHT = 295097  # known synced height


def rpc_call(url: str, method: str, params: list | None = None,
             timeout: float = 20.0) -> tuple:
    """Low-level JSON-RPC call. Returns (result, error_string)."""
    payload = json.dumps({
        "jsonrpc": "2.0",
        "method": method,
        "params": params or [],
        "id": 1,
    })
    parsed = urlparse(url)
    host = parsed.hostname
    port = parsed.port or 80
    try:
        conn = http.client.HTTPConnection(host, port, timeout=timeout)
        conn.request("POST", parsed.path or "/",
                     payload, {"Content-Type": "application/json"})
        resp = conn.getresponse()
        raw = resp.read()
        conn.close()

        # Handle gzip
        if raw[:2] == b'\x1f\x8b':
            raw = gzip.decompress(raw)

        # Strip any leading non-JSON bytes (HTTP chunked framing artifacts)
        start = raw.find(b'{')
        if start < 0:
            return None, f"No JSON in response ({len(raw)} bytes)"
        raw = raw[start:]

        data = json.loads(raw)
        return data.get("result"), data.get("error")
    except Exception as exc:
        return None, str(exc)


def get_stateroot(url: str, index: int, retries: int = 3) -> str | None:
    """Return the roothash for a given block index, or None on failure."""
    for attempt in range(retries):
        result, err = rpc_call(url, "getstateroot", [index])
        if result and isinstance(result, dict) and "roothash" in result:
            return result["roothash"]
        if attempt < retries - 1:
            time.sleep(0.5 * (attempt + 1))
    return None


def get_block_count(url: str) -> int | None:
    result, err = rpc_call(url, "getblockcount", [])
    if isinstance(result, int):
        return result
    return None


def get_stateroot_with_fallback(index: int, reference: str) -> str | None:
    """Try the primary reference, then fall back to other seeds."""
    root = get_stateroot(reference, index, retries=2)
    if root:
        return root
    for seed in FALLBACK_SEEDS:
        if seed == reference:
            continue
        root = get_stateroot(seed, index, retries=1)
        if root:
            return root
    return None


def wait_for_rpc(url: str, timeout_secs: float = 120.0) -> bool:
    """Wait for the RPC endpoint to become responsive."""
    deadline = time.time() + timeout_secs
    while time.time() < deadline:
        try:
            result, err = rpc_call(url, "getversion", timeout=5.0)
            if result is not None:
                return True
        except Exception:
            pass
        time.sleep(2)
    return False


def start_local_node(repo_dir: str) -> subprocess.Popen | None:
    """Start the neo-node and return the Popen handle."""
    binary = os.path.join(repo_dir, "target/release/neo-node")
    config = os.path.join(repo_dir, "config/mainnet-stateroot.toml")
    if not os.path.isfile(binary):
        print(f"ERROR: binary not found at {binary}")
        return None
    if not os.path.isfile(config):
        print(f"ERROR: config not found at {config}")
        return None

    env = os.environ.copy()
    env["NEO_LOG_LEVEL"] = "warn"

    print(f"Starting neo-node: {binary} --config {config} --enable-stateroot")
    proc = subprocess.Popen(
        [binary, "--config", config, "--enable-stateroot"],
        cwd=repo_dir,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return proc


def binary_search_divergence(local: str, reference: str,
                             low: int, high: int) -> int | None:
    """
    Binary search for the first block where state roots differ.
    Precondition: roots match at `low`, differ at `high`.
    Returns the exact block where divergence first occurs.
    """
    print(f"\n--- Binary search: low={low}, high={high} ---")
    iterations = 0

    while low < high:
        mid = (low + high) // 2
        iterations += 1

        local_root = get_stateroot(local, mid)
        if local_root is None:
            print(f"  [{iterations}] block {mid}: local returned None, "
                  "trying nearby blocks...")
            # Try to work around missing blocks
            found = False
            for offset in range(1, 5):
                if mid + offset <= high:
                    lr = get_stateroot(local, mid + offset)
                    rr = get_stateroot_with_fallback(mid + offset, reference)
                    if lr and rr:
                        if lr == rr:
                            low = mid + offset + 1
                        else:
                            high = mid + offset
                        found = True
                        break
            if not found:
                print(f"  Could not get local root near block {mid}")
                return None
            continue

        ref_root = get_stateroot_with_fallback(mid, reference)
        if ref_root is None:
            print(f"  [{iterations}] block {mid}: reference returned None, retrying...")
            time.sleep(2)
            ref_root = get_stateroot_with_fallback(mid, reference)
            if ref_root is None:
                print(f"  Could not get reference root at block {mid}")
                return None

        match = local_root == ref_root
        status = "MATCH" if match else "DIFFER"
        print(f"  [{iterations}] block {mid}: {status}")
        if not match:
            print(f"         local : {local_root}")
            print(f"         ref   : {ref_root}")

        if match:
            low = mid + 1
        else:
            high = mid

    return low


def linear_verify(local: str, reference: str,
                  start: int, count: int = 10) -> list[dict]:
    """Verify a range of blocks around the divergence point."""
    results = []
    for i in range(max(0, start - 2), start + count):
        lr = get_stateroot(local, i)
        rr = get_stateroot_with_fallback(i, reference)
        match = (lr == rr) if (lr and rr) else None
        results.append({
            "block": i,
            "local": lr,
            "reference": rr,
            "match": match,
        })
    return results


def main():
    parser = argparse.ArgumentParser(
        description="Find the first block where neo-rs state root diverges "
                    "from the C# reference node"
    )
    parser.add_argument("--local", default=LOCAL_RPC,
                        help=f"Local RPC endpoint (default: {LOCAL_RPC})")
    parser.add_argument("--reference", default=REFERENCE_RPC,
                        help=f"Reference RPC endpoint (default: {REFERENCE_RPC})")
    parser.add_argument("--max-height", type=int, default=MAX_HEIGHT,
                        help=f"Maximum block height to search (default: {MAX_HEIGHT})")
    parser.add_argument("--no-start-node", action="store_true",
                        help="Do not attempt to start the local node")
    parser.add_argument("--repo-dir", default="/home/neo/git/neo-rs",
                        help="Path to neo-rs repo")
    args = parser.parse_args()

    repo_dir = args.repo_dir
    local_url = args.local
    ref_url = args.reference
    max_height = args.max_height
    node_proc = None

    # Check if local RPC is already up
    print(f"Checking local RPC at {local_url}...")
    bc = get_block_count(local_url)
    if bc is not None:
        print(f"Local node already running. Block count: {bc}")
    elif not args.no_start_node:
        print("Local node not responding. Starting it...")
        node_proc = start_local_node(repo_dir)
        if node_proc is None:
            sys.exit(1)
        print(f"Waiting for RPC on {local_url} (up to 120s)...")
        if not wait_for_rpc(local_url, timeout_secs=120):
            print("ERROR: RPC did not become ready in time.")
            node_proc.terminate()
            sys.exit(1)
        bc = get_block_count(local_url)
        print(f"Local node RPC ready. Block count: {bc}")
    else:
        print("ERROR: Local node not responding and --no-start-node set.")
        sys.exit(1)

    # Determine the search range
    if bc is not None and bc - 1 < max_height:
        max_height = bc - 1
        print(f"Adjusted max height to {max_height} (local block count)")

    print(f"\nSearch range: 0 .. {max_height}")
    print(f"Reference: {ref_url}")

    # Step 1: Verify endpoints work
    print("\nStep 1: Verifying endpoints...")
    local_root_0 = get_stateroot(local_url, 0)
    ref_root_0 = get_stateroot_with_fallback(0, ref_url)
    print(f"  Block 0 local : {local_root_0}")
    print(f"  Block 0 ref   : {ref_root_0}")
    if local_root_0 is None:
        print("ERROR: Cannot get local state root for block 0")
        if node_proc:
            node_proc.terminate()
        sys.exit(1)
    if ref_root_0 is None:
        print("ERROR: Cannot get reference state root for block 0")
        if node_proc:
            node_proc.terminate()
        sys.exit(1)

    if local_root_0 != ref_root_0:
        print("\n" + "=" * 60)
        print("DIVERGENCE AT BLOCK 0 - genesis state root differs!")
        print("=" * 60)
        print(f"\n  local  block 0: {local_root_0}")
        print(f"  ref    block 0: {ref_root_0}")

        # Show a few more blocks for context
        print("\nFirst 10 blocks comparison:")
        details = linear_verify(local_url, ref_url, 0, count=10)
        print(f"\n{'Block':>8}  {'Match':>5}  {'Local Root':>66}  {'Reference Root':>66}")
        print("-" * 160)
        for d in details:
            m = "YES" if d["match"] else ("NO" if d["match"] is False else "???")
            lr = d["local"] or "None"
            rr = d["reference"] or "None"
            print(f"{d['block']:>8}  {m:>5}  {lr:>66}  {rr:>66}")

        # Also check stateheight
        result, _ = rpc_call(local_url, "getstateheight")
        if result:
            print(f"\nLocal state height info: {json.dumps(result)}")

        # Write result
        result_file = os.path.join(repo_dir, "divergence_result.json")
        with open(result_file, "w") as f:
            json.dump({
                "first_divergent_block": 0,
                "details": details,
                "local_endpoint": local_url,
                "reference_endpoint": ref_url,
                "note": "Genesis state root already differs - fundamental MPT computation issue",
            }, f, indent=2)
        print(f"\nResults written to {result_file}")

        if node_proc:
            node_proc.terminate()
        return 0

    # Step 2: Check if max height matches
    print("\nStep 2: Checking state root at max height...")
    local_root_max = get_stateroot(local_url, max_height)
    ref_root_max = get_stateroot_with_fallback(max_height, ref_url)
    print(f"  Block {max_height} local : {local_root_max}")
    print(f"  Block {max_height} ref   : {ref_root_max}")

    if local_root_max is None:
        # Try backing off to find highest available
        print("  Local root at max height is None, searching for highest available...")
        for h in range(max_height, max(0, max_height - 100), -1):
            lr = get_stateroot(local_url, h)
            if lr is not None:
                max_height = h
                local_root_max = lr
                ref_root_max = get_stateroot_with_fallback(h, ref_url)
                print(f"  Found local root at block {h}")
                break
        else:
            print("ERROR: Cannot find any local state root near max height")
            if node_proc:
                node_proc.terminate()
            sys.exit(1)

    if local_root_max == ref_root_max:
        print(f"\nAll state roots match through block {max_height}. No divergence found!")
        if node_proc:
            node_proc.terminate()
        sys.exit(0)

    print(f"\nDivergence confirmed: roots differ at block {max_height}")
    print(f"  local : {local_root_max}")
    print(f"  ref   : {ref_root_max}")

    # Step 3: Coarse binary search with exponential probing first
    print("\nStep 3: Exponential probing to narrow range...")
    # Check powers of 2 to quickly find rough range
    probe_points = []
    p = 1
    while p < max_height:
        probe_points.append(p)
        p *= 2
    probe_points.append(max_height)

    last_match = 0
    first_diff = max_height
    for probe in probe_points:
        if probe > max_height:
            probe = max_height
        lr = get_stateroot(local_url, probe)
        rr = get_stateroot_with_fallback(probe, ref_url)
        if lr is None or rr is None:
            print(f"  Probe {probe}: skipped (null response)")
            continue
        if lr == rr:
            last_match = probe
            print(f"  Probe {probe:>8}: MATCH")
        else:
            first_diff = probe
            print(f"  Probe {probe:>8}: DIFFER")
            print(f"         local : {lr}")
            print(f"         ref   : {rr}")
            break

    print(f"\nNarrowed range: {last_match} .. {first_diff}")

    # Step 4: Binary search in the narrowed range
    print("\nStep 4: Binary search for exact divergence point...")
    diverge_block = binary_search_divergence(
        local_url, ref_url, last_match, first_diff
    )

    if diverge_block is None:
        print("ERROR: Binary search failed to find exact divergence point")
        if node_proc:
            node_proc.terminate()
        sys.exit(1)

    print(f"\n{'=' * 60}")
    print(f"FIRST DIVERGENCE AT BLOCK: {diverge_block}")
    print(f"{'=' * 60}")

    # Step 5: Show context around the divergence
    print(f"\nStep 5: Verifying with linear scan around block {diverge_block}...")
    details = linear_verify(local_url, ref_url, diverge_block, count=5)
    print(f"\n{'Block':>8}  {'Match':>5}  {'Local Root':>66}  {'Reference Root':>66}")
    print("-" * 160)
    for d in details:
        m = "YES" if d["match"] else ("NO" if d["match"] is False else "???")
        lr = d["local"] or "None"
        rr = d["reference"] or "None"
        marker = " <<<" if d["match"] is False and d["block"] == diverge_block else ""
        print(f"{d['block']:>8}  {m:>5}  {lr:>66}  {rr:>66}{marker}")

    # Write result to file
    result_file = os.path.join(repo_dir, "divergence_result.json")
    with open(result_file, "w") as f:
        json.dump({
            "first_divergent_block": diverge_block,
            "details": details,
            "local_endpoint": local_url,
            "reference_endpoint": ref_url,
            "max_height_searched": max_height,
        }, f, indent=2)
    print(f"\nResults written to {result_file}")

    # Cleanup
    if node_proc:
        print("\nStopping local node...")
        node_proc.terminate()
        try:
            node_proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            node_proc.kill()

    return diverge_block


if __name__ == "__main__":
    result = main()
    sys.exit(0 if result is not None else 1)
