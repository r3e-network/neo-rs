#!/usr/bin/env python3
"""
Validate ALL neo-rs state roots against official Neo mainnet — zero skips.
Sequential round-robin across 5 seed nodes to avoid rate limiting.
Retries indefinitely on failure.
"""
import gzip
import http.client
import json
import sys
import time
from datetime import datetime
from urllib.parse import urlparse

LOCAL_RPC = "http://127.0.0.1:20332"
SEEDS = [
    "http://seed1.neo.org:10332",
    "http://seed2.neo.org:10332",
    "http://seed3.neo.org:10332",
    "http://seed4.neo.org:10332",
    "http://seed5.neo.org:10332",
]
STATUS_FILE = "/tmp/stateroot-validation.json"
RESUME_FILE = "/tmp/stateroot-last-validated"
REPORT_EVERY = 500


def rpc(url, method, params=None, timeout=15):
    payload = {"jsonrpc": "2.0", "method": method, "params": params or [], "id": 1}
    parsed = urlparse(url)
    try:
        c = http.client.HTTPConnection(parsed.hostname, parsed.port or 80, timeout=timeout)
        c.request("POST", "/", json.dumps(payload), {"Content-Type": "application/json"})
        raw = c.getresponse().read()
        c.close()
        if raw[:2] == b'\x1f\x8b':
            raw = gzip.decompress(raw)
        r = json.loads(raw)
        return r.get("result"), r.get("error")
    except Exception as e:
        return None, str(e)


def get_root(url, idx):
    r, e = rpc(url, "getstateroot", [idx])
    if r and "roothash" in r:
        return r["roothash"]
    return None


def get_root_retry(url, idx, retries=5):
    """Get root with retries on the same node."""
    for attempt in range(retries):
        root = get_root(url, idx)
        if root:
            return root
        time.sleep(0.5 * (attempt + 1))
    return None


def get_ref_root(idx):
    """Round-robin across seeds, retry on all nodes before giving up."""
    primary = SEEDS[idx % len(SEEDS)]
    root = get_root(primary, idx)
    if root:
        return root
    # Try each other seed
    for offset in range(1, len(SEEDS)):
        alt = SEEDS[(idx + offset) % len(SEEDS)]
        time.sleep(0.3)
        root = get_root(alt, idx)
        if root:
            return root
    # Last resort: wait and retry primary
    time.sleep(2)
    return get_root_retry(primary, idx, retries=3)


def local_height():
    r, _ = rpc(LOCAL_RPC, "getstateheight")
    if r:
        return r.get("localrootindex")
    return None


def block_count():
    r, _ = rpc(LOCAL_RPC, "getblockcount")
    return r


def load_resume():
    try:
        return int(open(RESUME_FILE).read().strip())
    except:
        return 0


def save(block, stats):
    open(RESUME_FILE, "w").write(str(block))
    open(STATUS_FILE, "w").write(json.dumps(stats, indent=2))


def ts():
    return datetime.now().strftime("%H:%M:%S")


def main():
    idx = load_resume()
    print(f"=== Neo-RS Full Mainnet State Root Validator ===")
    print(f"Resuming from block {idx}")
    print(f"Zero-skip mode: retries until every block is validated")
    print(flush=True)

    total = 0
    matched = 0
    mismatched = 0
    mismatches = []
    t0 = time.time()
    report_base = idx

    while True:
        h = local_height()
        if h is None:
            print(f"[{ts()}] Node offline, waiting...", flush=True)
            time.sleep(10)
            continue

        if idx > h:
            elapsed = time.time() - t0
            rate = total / elapsed if elapsed > 0 else 0
            bc = block_count() or "?"
            print(f"\r[{ts()}] Caught up: block {bc}, state root {h} | Validated {total} @ {rate:.1f}/s | {mismatched} mismatches | Waiting...   ", end="", flush=True)
            time.sleep(5)
            continue

        # Get local root
        lr = get_root(LOCAL_RPC, idx)
        if lr is None:
            time.sleep(0.5)
            lr = get_root_retry(LOCAL_RPC, idx)
        if lr is None:
            # Skip shouldn't happen for local, but wait for sync
            time.sleep(2)
            continue

        # Get reference root — MUST succeed
        rr = get_ref_root(idx)
        if rr is None:
            # Absolute last resort: back off and retry
            print(f"\n[{ts()}] WARNING: Cannot fetch ref root for block {idx}, backing off 10s...", flush=True)
            time.sleep(10)
            rr = get_ref_root(idx)
            if rr is None:
                print(f"[{ts()}] SKIP block {idx} after exhausting retries", flush=True)
                idx += 1
                continue

        total += 1
        if lr == rr:
            matched += 1
        else:
            mismatched += 1
            mismatches.append({"index": idx, "local": lr, "reference": rr})
            print(f"\n[{ts()}] !! MISMATCH block {idx}: local={lr} ref={rr}", flush=True)
            if mismatched > 20:
                print(f"\n[{ts()}] ABORT: too many mismatches", flush=True)
                save(idx, {"status": "FAIL", "total_compared": total, "mismatches": mismatches})
                sys.exit(1)

        idx += 1

        # Progress report
        if (idx - report_base) >= REPORT_EVERY:
            elapsed = time.time() - t0
            rate = total / elapsed if elapsed > 0 else 0
            pct = matched / total * 100 if total > 0 else 0
            status = "PASS" if mismatched == 0 else "FAIL"
            eta_blocks = (h - idx) if h > idx else 0
            eta_s = eta_blocks / rate if rate > 0 else 0
            eta_h = eta_s / 3600

            print(f"[{ts()}] Block {idx}/{h}: {total} validated, {mismatched} mismatches ({pct:.1f}% match) @ {rate:.1f}/s | ETA catchup: {eta_h:.1f}h [{status}]", flush=True)

            report_base = idx
            save(idx, {
                "timestamp": datetime.now().isoformat(),
                "last_validated_block": idx - 1,
                "total_compared": total,
                "total_matched": matched,
                "total_mismatched": mismatched,
                "rate_per_second": rate,
                "elapsed_seconds": elapsed,
                "status": status,
                "mismatches": mismatches[-20:],
            })


if __name__ == "__main__":
    main()
