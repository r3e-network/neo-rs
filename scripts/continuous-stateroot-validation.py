#!/usr/bin/env python3
"""
Continuous state root validation for neo-rs.

Compares state roots from the local neo-rs node against a C# reference node
for every synced block. Runs continuously, reporting progress and mismatches.

Usage:
    python3 scripts/continuous-stateroot-validation.py [options]

Options:
    --local URL          Local neo-rs RPC (default: http://127.0.0.1:10332)
    --reference URL      Reference C# node (default: http://seed1.neo.org:10332)
    --start N            Start block index (default: 0)
    --batch N            Batch size for comparison (default: 500)
    --workers N          Parallel fetch workers (default: 8)
    --output FILE        Write results to JSON file
    --poll-interval N    Seconds between sync polls (default: 5)
"""

import argparse
import gzip
import http.client
import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime
from urllib.parse import urlparse


def rpc_call(url, method, params=None, timeout=30):
    payload = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params or [],
        "id": 1
    }
    body = json.dumps(payload)
    parsed = urlparse(url)
    try:
        conn = http.client.HTTPConnection(parsed.hostname, parsed.port or 80, timeout=timeout)
        conn.request("POST", parsed.path or "/", body, {"Content-Type": "application/json"})
        resp = conn.getresponse()
        raw = resp.read()
        conn.close()
        if raw[:2] == b'\x1f\x8b':
            raw = gzip.decompress(raw)
        result = json.loads(raw.decode("utf-8"))
        if "error" in result:
            return None, result["error"]
        return result.get("result"), None
    except Exception as e:
        return None, str(e)


def get_state_root(url, index, timeout=30):
    result, err = rpc_call(url, "getstateroot", [index], timeout)
    if err:
        return None, err
    if result and "roothash" in result:
        return result["roothash"], None
    return None, f"unexpected: {result}"


def get_state_height(url, timeout=10):
    result, err = rpc_call(url, "getstateheight", [], timeout)
    if err:
        return None, None, err
    if result:
        return result.get("localrootindex", 0), result.get("validatedrootindex", 0), None
    return None, None, "empty"


def get_block_count(url, timeout=10):
    result, err = rpc_call(url, "getblockcount", [], timeout)
    if err:
        return None, err
    return result, None


def fetch_roots_batch(url, start, end, workers=8):
    roots = {}
    with ThreadPoolExecutor(max_workers=workers) as exe:
        futs = {exe.submit(get_state_root, url, i): i for i in range(start, end + 1)}
        for f in as_completed(futs):
            idx = futs[f]
            root, err = f.result()
            roots[idx] = root
    return roots


def status_file(path, data):
    if path:
        with open(path, "w") as f:
            json.dump(data, f, indent=2)


def main():
    p = argparse.ArgumentParser(description="Continuous state root validator")
    p.add_argument("--local", default="http://127.0.0.1:10332")
    p.add_argument("--reference", default="http://seed1.neo.org:10332")
    p.add_argument("--start", type=int, default=0)
    p.add_argument("--batch", type=int, default=500)
    p.add_argument("--workers", type=int, default=8)
    p.add_argument("--output", default="/tmp/stateroot-validation.json")
    p.add_argument("--poll-interval", type=int, default=5)
    args = p.parse_args()

    print(f"=== Neo-RS Continuous State Root Validator ===")
    print(f"Local:     {args.local}")
    print(f"Reference: {args.reference}")
    print(f"Start:     {args.start}")
    print(f"Batch:     {args.batch}")
    print()

    # Verify reference node
    ref_count, err = get_block_count(args.reference)
    if err:
        print(f"ERROR: Cannot reach reference: {err}")
        sys.exit(1)
    print(f"Reference node at block {ref_count}")

    total_compared = 0
    total_matched = 0
    total_mismatched = 0
    mismatches = []
    last_compared = args.start - 1
    start_time = time.time()

    while True:
        # Check local node
        local_height, validated_height, err = get_state_height(args.local)
        if err:
            elapsed = time.time() - start_time
            print(f"\r[{datetime.now().strftime('%H:%M:%S')}] Waiting for local node... ({err}) [{total_compared} compared, {total_mismatched} mismatches, {elapsed:.0f}s]", end="", flush=True)
            time.sleep(args.poll_interval)
            continue

        if local_height is None:
            local_height = 0

        # Also get block count
        block_count, _ = get_block_count(args.local)
        block_count = block_count or 0

        # Determine range
        compare_start = last_compared + 1
        compare_end = min(local_height, compare_start + args.batch - 1)

        if compare_start > compare_end:
            elapsed = time.time() - start_time
            rate = total_compared / elapsed if elapsed > 0 else 0
            print(f"\r[{datetime.now().strftime('%H:%M:%S')}] Block {block_count} | State root {local_height} | Compared {total_compared} ({rate:.0f}/s) | {total_mismatched} mismatches | Waiting...", end="", flush=True)
            time.sleep(args.poll_interval)
            continue

        print(f"\n[{datetime.now().strftime('%H:%M:%S')}] Comparing blocks {compare_start}-{compare_end} (local state at {local_height}, block {block_count})...")

        # Fetch reference and local roots in parallel
        ref_roots = fetch_roots_batch(args.reference, compare_start, compare_end, args.workers)
        local_roots = fetch_roots_batch(args.local, compare_start, compare_end, args.workers)

        batch_match = 0
        batch_mismatch = 0
        batch_error = 0

        for idx in range(compare_start, compare_end + 1):
            local_root = local_roots.get(idx)
            ref_root = ref_roots.get(idx)

            if local_root is None or ref_root is None:
                batch_error += 1
                continue

            total_compared += 1
            if local_root == ref_root:
                batch_match += 1
                total_matched += 1
            else:
                batch_mismatch += 1
                total_mismatched += 1
                mismatches.append({
                    "index": idx,
                    "local": local_root,
                    "reference": ref_root
                })
                print(f"  MISMATCH at block {idx}:")
                print(f"    Local:     {local_root}")
                print(f"    Reference: {ref_root}")

        last_compared = compare_end
        elapsed = time.time() - start_time
        rate = total_compared / elapsed if elapsed > 0 else 0
        pct = (total_matched / total_compared * 100) if total_compared > 0 else 0

        print(f"  Batch: {batch_match} ok, {batch_mismatch} mismatch, {batch_error} error | Total: {total_matched}/{total_compared} ({pct:.1f}%) @ {rate:.0f}/s")

        # Write status
        status_file(args.output, {
            "timestamp": datetime.now().isoformat(),
            "last_compared": last_compared,
            "total_compared": total_compared,
            "total_matched": total_matched,
            "total_mismatched": total_mismatched,
            "rate_per_second": rate,
            "elapsed_seconds": elapsed,
            "mismatches": mismatches[-100:],
            "status": "PASS" if total_mismatched == 0 else "FAIL"
        })

        if total_mismatched > 10:
            print(f"\n!!! TOO MANY MISMATCHES ({total_mismatched}) - stopping !!!")
            break


if __name__ == "__main__":
    main()
