#!/usr/bin/env python3
import argparse
import json
import urllib.request
import time
import gzip
import os
from concurrent.futures import ThreadPoolExecutor, as_completed

def rpc_call(url: str, method: str, params: list, timeout=20, retries=10):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode("utf-8")
    req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"}, method="POST")
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
                if raw.startswith(b"\x1f\x8b"):
                    raw = gzip.decompress(raw)
                result = json.loads(raw.decode("utf-8"))
            if "error" in result:
                return None, result["error"]
            return result.get("result"), None
        except Exception as e:
            if attempt == retries - 1:
                return None, str(e)
            time.sleep(2)
    return None, "Max retries reached"

def get_stateroot(url, height):
    res, err = rpc_call(url, "getstateroot", [height])
    if err or not res:
        return None, None, err
    return height, res.get("roothash"), None

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--csharp", default="http://seed1.neo.org:10332")
    parser.add_argument("--target", type=int, default=500000)
    parser.add_argument("--output", default="data/reference_stateroots.jsonl")
    parser.add_argument("--workers", type=int, default=10)
    args = parser.parse_args()

    print(f"Downloading state roots up to {args.target} from {args.csharp}")

    # Load existing heights to avoid re-fetching
    existing_heights = set()
    if os.path.exists(args.output):
        with open(args.output, "r") as f:
            for line in f:
                if line.strip():
                    try:
                        data = json.loads(line)
                        existing_heights.add(data["height"])
                    except:
                        pass

    print(f"Found {len(existing_heights)} existing state roots in {args.output}")

    batch_size = 100

    # Open file in append mode
    with open(args.output, "a") as f:
        for start in range(0, args.target, batch_size):
            end = min(start + batch_size, args.target)

            heights_to_fetch = [h for h in range(start, end) if h not in existing_heights]

            if not heights_to_fetch:
                continue

            futures = {}
            results = []

            with ThreadPoolExecutor(max_workers=args.workers) as executor:
                for h in heights_to_fetch:
                    futures[executor.submit(get_stateroot, args.csharp, h)] = h

                for fut in as_completed(futures):
                    h = futures[fut]
                    height, roothash, err = fut.result()
                    if err:
                        print(f"\nError fetching height {h}: {err}")
                        # We will just write None or skip, let's skip so we can retry later
                    else:
                        results.append((height, roothash))

            # Write to file
            for height, roothash in sorted(results):
                record = {"height": height, "roothash": roothash}
                f.write(json.dumps(record) + "\n")
                f.flush()

            print(f"\rDownloaded up to {end} state roots", end="", flush=True)

    print(f"\nDone downloading state roots to {args.output}")

if __name__ == "__main__":
    main()
