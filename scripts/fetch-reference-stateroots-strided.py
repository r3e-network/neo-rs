#!/usr/bin/env python3
"""Fetch MainNet reference state roots at a fixed stride from the public seeds.

`download_stateroots_parallel.py` fetches every height, which is impractical
across the full 11.49M-block chain. Parity evidence does not need every height:
a state root commits to the whole state at that height, so a matching root at
H and at H+stride means every intermediate transition produced identical state.
This fetches a strided ladder plus any explicitly requested heights, writing
the same JSONL shape (`{"height": int, "root": "0x..."}`) that the comparison
tooling reads.

Resumable: existing heights in --output are kept and skipped.
"""
import argparse
import gzip
import json
import os
import sys
import threading
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

SEEDS = [f"http://seed{i}.neo.org:10332" for i in (1, 2, 3, 4, 5)]

_write_lock = threading.Lock()


def rpc_state_root(url, height, timeout=20, retries=4):
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": "getstateroot", "params": [height]}
    ).encode()
    for attempt in range(retries):
        try:
            req = urllib.request.Request(
                url,
                data=payload,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
            if raw.startswith(b"\x1f\x8b"):
                raw = gzip.decompress(raw)
            result = json.loads(raw.decode())
            if "error" in result:
                return None, str(result["error"])
            return result["result"]["roothash"], None
        except Exception as exc:  # noqa: BLE001 - report and retry
            if attempt == retries - 1:
                return None, f"{type(exc).__name__}: {exc}"
            time.sleep(1 + 2 * attempt)
    return None, "exhausted retries"


def load_existing(path):
    have = {}
    if not os.path.exists(path):
        return have
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            height = rec.get("height")
            root = rec.get("root") or rec.get("roothash")
            if isinstance(height, int) and isinstance(root, str):
                have[height] = root
    return have


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--start", type=int, default=0)
    ap.add_argument("--end", type=int, required=True, help="inclusive upper bound")
    ap.add_argument("--stride", type=int, default=1000)
    ap.add_argument("--extra", type=int, nargs="*", default=[])
    ap.add_argument("--output", default="data/reference_stateroots_strided.jsonl")
    ap.add_argument("--workers", type=int, default=20)
    ap.add_argument("--seeds", nargs="*", default=SEEDS)
    ap.add_argument("--progress-every", type=int, default=250)
    args = ap.parse_args()

    wanted = set(range(args.start, args.end + 1, args.stride))
    wanted.add(args.end)
    wanted.update(h for h in args.extra if args.start <= h <= args.end)

    have = load_existing(args.output)
    todo = sorted(h for h in wanted if h not in have)
    print(
        f"target heights={len(wanted)} already_have={len(have)} to_fetch={len(todo)}",
        flush=True,
    )
    if not todo:
        return 0

    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)
    out = open(args.output, "a", buffering=1, encoding="utf-8")
    seeds = args.seeds
    done = [0]
    failed = []

    def task(idx_height):
        idx, height = idx_height
        seed = seeds[idx % len(seeds)]
        root, err = rpc_state_root(seed, height)
        if err:
            # one retry on a different seed before giving up
            root, err = rpc_state_root(seeds[(idx + 1) % len(seeds)], height)
        with _write_lock:
            done[0] += 1
            if err:
                failed.append((height, err))
                print(f"h={height}: {err}", file=sys.stderr, flush=True)
            else:
                out.write(json.dumps({"height": height, "root": root}) + "\n")
            if done[0] % args.progress_every == 0:
                print(
                    f"progress {done[0]}/{len(todo)} failed={len(failed)}",
                    flush=True,
                )

    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        list(ex.map(task, enumerate(todo)))

    out.close()
    print(f"fetched={done[0] - len(failed)} failed={len(failed)}", flush=True)
    if failed:
        print("first failures: " + str(failed[:5]), flush=True)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
