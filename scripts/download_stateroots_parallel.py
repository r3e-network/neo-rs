#!/usr/bin/env python3
"""Download mainnet state roots in parallel across multiple Neo seed nodes.

Each seed gets its own contiguous height slice; per-seed concurrency is bounded
by --per-seed-workers. Results stream to per-seed JSONL files and are merged
into the canonical file (default data/reference_stateroots.jsonl) at exit, with
duplicate heights collapsed.

Resumable: an existing canonical file is read first and only missing heights
are fetched.
"""
import argparse
import json
import os
import sys
import time
import urllib.request
import gzip
from concurrent.futures import ThreadPoolExecutor, as_completed

SEEDS = [f"http://seed{i}.neo.org:10332" for i in (1, 2, 3, 4, 5)]


def rpc_state_root(url, height, timeout=15, retries=5):
    payload = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "getstateroot", "params": [height]
    }).encode()
    req = urllib.request.Request(
        url, data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
                if raw.startswith(b"\x1f\x8b"):
                    raw = gzip.decompress(raw)
                result = json.loads(raw.decode())
            if "error" in result:
                return None, result["error"]
            return result["result"]["roothash"], None
        except Exception as e:
            if attempt == retries - 1:
                return None, str(e)
            time.sleep(1 + attempt)
    return None, "exhausted retries"


def worker_for_seed(seed_url, heights, out_path, workers, progress):
    """Fetch state roots for `heights` from `seed_url`. Stream to out_path."""
    with open(out_path, "a", buffering=1) as out:
        with ThreadPoolExecutor(max_workers=workers) as ex:
            futs = {ex.submit(rpc_state_root, seed_url, h): h for h in heights}
            for fut in as_completed(futs):
                h = futs[fut]
                root, err = fut.result()
                if err:
                    print(f"[{seed_url}] h={h}: {err}", file=sys.stderr)
                    continue
                out.write(json.dumps({"height": h, "roothash": root}) + "\n")
                progress[seed_url] += 1


def read_existing_heights(path):
    have = set()
    if not os.path.exists(path):
        return have
    with open(path) as f:
        for line in f:
            if not line.strip():
                continue
            try:
                have.add(json.loads(line)["height"])
            except Exception:
                pass
    return have


def merge_into_canonical(canonical, per_seed_paths):
    """Read canonical + all per-seed files, dedupe by height, write back sorted."""
    by_height = {}
    for src in [canonical] + per_seed_paths:
        if not os.path.exists(src):
            continue
        with open(src) as f:
            for line in f:
                if not line.strip():
                    continue
                try:
                    rec = json.loads(line)
                    by_height[rec["height"]] = rec["roothash"]
                except Exception:
                    pass
    tmp = canonical + ".tmp"
    with open(tmp, "w") as f:
        for h in sorted(by_height):
            f.write(json.dumps({"height": h, "roothash": by_height[h]}) + "\n")
    os.replace(tmp, canonical)
    print(f"merged {len(by_height)} unique heights → {canonical}")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--start", type=int, required=True)
    p.add_argument("--target", type=int, required=True, help="exclusive upper bound")
    p.add_argument("--output", default="data/reference_stateroots.jsonl")
    p.add_argument("--per-seed-workers", type=int, default=8)
    p.add_argument("--seeds", nargs="*", default=SEEDS)
    args = p.parse_args()

    existing = read_existing_heights(args.output)
    print(f"existing heights in {args.output}: {len(existing)}")

    missing = [h for h in range(args.start, args.target) if h not in existing]
    if not missing:
        print(f"nothing to do: {args.start}..{args.target} fully covered")
        return
    print(f"fetching {len(missing)} missing heights using {len(args.seeds)} seeds × {args.per_seed_workers} workers")

    # Partition missing heights into N round-robin slices, one per seed
    slices = [[] for _ in args.seeds]
    for i, h in enumerate(missing):
        slices[i % len(args.seeds)].append(h)

    per_seed_paths = [
        f"{args.output}.part{i}-{os.getpid()}" for i in range(len(args.seeds))
    ]
    # Clear any prior partial output files
    for p_ in per_seed_paths:
        if os.path.exists(p_):
            os.remove(p_)

    progress = {seed: 0 for seed in args.seeds}
    start_time = time.time()

    # Use raw threads instead of ThreadPoolExecutor for the outer fan-out so we
    # can force-exit even if some inner thread is stuck after all data has been
    # written. (Prior run hung in ThreadPoolExecutor.__exit__ shutdown with all
    # futures done; the merge step never ran, requiring manual recovery.)
    import threading
    threads = []
    for i in range(len(args.seeds)):
        t = threading.Thread(
            target=worker_for_seed,
            args=(args.seeds[i], slices[i], per_seed_paths[i],
                  args.per_seed_workers, progress),
            daemon=True,
        )
        t.start()
        threads.append(t)

    while any(t.is_alive() for t in threads):
        time.sleep(20)
        elapsed = time.time() - start_time
        total = sum(progress.values())
        rate = total / elapsed if elapsed > 0 else 0
        print(f"  +{total} fetched in {int(elapsed)}s ({rate:.1f}/s) | per-seed: " +
              ", ".join(f"{u.split('//')[1].split('.')[0]}={progress[u]}" for u in args.seeds),
              flush=True)

    for t in threads:
        t.join(timeout=30)

    merge_into_canonical(args.output, per_seed_paths)
    for p_ in per_seed_paths:
        if os.path.exists(p_):
            os.remove(p_)
    print(f"done in {int(time.time() - start_time)}s", flush=True)
    # Daemon-thread inner pools may keep extra worker threads alive even after
    # all heights have been fetched. Force exit so background invocations don't
    # hang indefinitely after the merge step completes.
    os._exit(0)


if __name__ == "__main__":
    main()
