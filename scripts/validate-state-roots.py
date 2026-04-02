#!/usr/bin/env python3
"""
State root validation script for neo-rs.

Compares state roots computed by the local neo-rs node against a reference
Neo N3 node (C# neo-cli or neogo) for every block during sync.

Usage:
    python3 scripts/validate-state-roots.py [--local URL] [--reference URL] [--start N] [--end N] [--batch SIZE]

The script polls the local node's getstateheight, then fetches and compares
state roots in batches against the reference node.
"""

import argparse
import json
import sys
import time
import urllib.request
import urllib.error
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime

import gzip

def rpc_call(url, method, params=None, timeout=30):
    """Make a JSON-RPC call."""
    payload = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params or [],
        "id": 1
    }
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json", "Accept-Encoding": "identity"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read()
            if raw.startswith(b"\x1f\x8b"):
                raw = gzip.decompress(raw)
            result = json.loads(raw.decode("utf-8"))
            if "error" in result:
                return None, result["error"]
            return result.get("result"), None
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, OSError) as e:
        return None, str(e)

def get_state_root(url, index, timeout=30):
    """Get state root for a specific block index."""
    result, err = rpc_call(url, "getstateroot", [index], timeout)
    if err:
        return None, err
    if result and "roothash" in result:
        return result["roothash"], None
    return None, f"unexpected response: {result}"

def get_state_height(url, timeout=10):
    """Get current state root heights from local node."""
    result, err = rpc_call(url, "getstateheight", [], timeout)
    if err:
        return None, None, err
    if result:
        return result.get("localrootindex", 0), result.get("validatedrootindex", 0), None
    return None, None, "empty response"

def get_block_count(url, timeout=10):
    """Get current block height."""
    result, err = rpc_call(url, "getblockcount", [], timeout)
    if err:
        return None, err
    return result, None

def fetch_reference_roots_batch(ref_url, start, end, workers=8):
    """Fetch reference state roots in parallel."""
    roots = {}
    with ThreadPoolExecutor(max_workers=workers) as executor:
        futures = {}
        for i in range(start, end + 1):
            futures[executor.submit(get_state_root, ref_url, i)] = i
        for future in as_completed(futures):
            idx = futures[future]
            root, err = future.result()
            if root:
                roots[idx] = root
            else:
                roots[idx] = None
    return roots

def main():
    parser = argparse.ArgumentParser(description="Validate neo-rs state roots against reference node")
    parser.add_argument("--local", default="http://127.0.0.1:10332", help="Local neo-rs RPC URL")
    parser.add_argument("--reference", default="http://seed1.neo.org:10332", help="Reference node RPC URL")
    parser.add_argument("--start", type=int, default=0, help="Start block index")
    parser.add_argument("--end", type=int, default=50000, help="End block index (0 = follow sync)")
    parser.add_argument("--batch", type=int, default=500, help="Batch size for comparison")
    parser.add_argument("--poll-interval", type=int, default=5, help="Seconds between sync polls")
    parser.add_argument("--workers", type=int, default=8, help="Parallel workers for reference fetches")
    parser.add_argument("--output", default=None, help="Output file for results (JSON)")
    args = parser.parse_args()

    print(f"=== Neo-RS State Root Validator ===")
    print(f"Local:     {args.local}")
    print(f"Reference: {args.reference}")
    print(f"Range:     {args.start} - {args.end}")
    print(f"Batch:     {args.batch}")
    print()

    # Check reference node is reachable
    ref_count, err = get_block_count(args.reference)
    if err:
        print(f"ERROR: Cannot reach reference node: {err}")
        sys.exit(1)
    print(f"Reference node at block {ref_count}")

    # Track results
    total_compared = 0
    total_matched = 0
    total_mismatched = 0
    mismatches = []
    errors = []
    last_compared = args.start - 1

    start_time = time.time()

    while last_compared < args.end:
        # Check local node sync progress
        local_height, validated_height, err = get_state_height(args.local)
        if err:
            # Node might not be up yet, wait
            print(f"\r[{datetime.now().strftime('%H:%M:%S')}] Waiting for local node... ({err})", end="", flush=True)
            time.sleep(args.poll_interval)
            continue

        if local_height is None:
            local_height = 0

        # Determine comparison range
        compare_start = last_compared + 1
        compare_end = min(local_height, args.end, compare_start + args.batch - 1)

        if compare_start > compare_end:
            # Need to wait for more blocks
            elapsed = time.time() - start_time
            rate = total_compared / elapsed if elapsed > 0 else 0
            print(f"\r[{datetime.now().strftime('%H:%M:%S')}] Synced to {local_height}, compared {total_compared} blocks ({rate:.0f}/s), waiting for more blocks...", end="", flush=True)
            time.sleep(args.poll_interval)
            continue

        # Fetch reference roots for this batch
        print(f"\n[{datetime.now().strftime('%H:%M:%S')}] Comparing blocks {compare_start}-{compare_end}...", flush=True)
        ref_roots = fetch_reference_roots_batch(args.reference, compare_start, compare_end, args.workers)

        # Compare each block
        batch_match = 0
        batch_mismatch = 0
        batch_error = 0

        for idx in range(compare_start, compare_end + 1):
            local_root, local_err = get_state_root(args.local, idx)
            ref_root = ref_roots.get(idx)

            if local_err:
                batch_error += 1
                errors.append({"index": idx, "error": f"local: {local_err}"})
                continue
            if ref_root is None:
                batch_error += 1
                errors.append({"index": idx, "error": "reference root unavailable"})
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
        print(f"  Batch: {batch_match} match, {batch_mismatch} mismatch, {batch_error} error | "
              f"Total: {total_matched}/{total_compared} match ({rate:.0f} blocks/s)")

        if total_mismatched > 0:
            print(f"\n  WARNING: {total_mismatched} mismatches found so far!")

    # Final summary
    elapsed = time.time() - start_time
    print(f"\n{'='*60}")
    print(f"VALIDATION COMPLETE")
    print(f"{'='*60}")
    print(f"Blocks compared:  {total_compared}")
    print(f"Matched:          {total_matched}")
    print(f"Mismatched:       {total_mismatched}")
    print(f"Errors:           {len(errors)}")
    print(f"Time:             {elapsed:.1f}s ({total_compared/elapsed:.0f} blocks/s)" if elapsed > 0 else "")
    print(f"Result:           {'PASS - 100% compatible' if total_mismatched == 0 else 'FAIL - divergences found'}")

    if mismatches:
        print(f"\nFirst 10 mismatches:")
        for m in mismatches[:10]:
            print(f"  Block {m['index']}: local={m['local']} ref={m['reference']}")

    # Write output file
    if args.output:
        report = {
            "timestamp": datetime.now().isoformat(),
            "local_url": args.local,
            "reference_url": args.reference,
            "range": {"start": args.start, "end": last_compared},
            "total_compared": total_compared,
            "total_matched": total_matched,
            "total_mismatched": total_mismatched,
            "errors": len(errors),
            "elapsed_seconds": elapsed,
            "mismatches": mismatches,
            "error_details": errors[:100],
            "result": "PASS" if total_mismatched == 0 else "FAIL"
        }
        with open(args.output, "w") as f:
            json.dump(report, f, indent=2)
        print(f"\nReport written to {args.output}")

    sys.exit(0 if total_mismatched == 0 else 1)

if __name__ == "__main__":
    main()
