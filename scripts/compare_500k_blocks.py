#!/usr/bin/env python3
import argparse
import json
import urllib.request
import time
import gzip
import os
from concurrent.futures import ThreadPoolExecutor, as_completed

def rpc_call(url: str, method: str, params: list, timeout=30, retries=10):
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

def get_block_info(url, height):
    block, err = rpc_call(url, "getblock", [height, True])
    if err or not block:
        return None, err

    tx_hashes = [tx.get("hash") for tx in block.get("tx", [])]
    return {
        "hash": block.get("hash"),
        "tx_count": len(tx_hashes),
        "tx_hashes": tx_hashes,
    }, None

def get_stateroot(url, height):
    res, err = rpc_call(url, "getstateroot", [height])
    if err or not res:
        return None, err
    return res.get("roothash"), None

def compare_height(rust_url, csharp_url, height, reference_roots):
    rust_block, r_err = get_block_info(rust_url, height)
    csharp_block, c_err = get_block_info(csharp_url, height)

    if r_err or c_err:
        return False, f"Block fetch error: Rust={r_err} C#={c_err}"

    if rust_block["hash"] != csharp_block["hash"]:
        return False, f"Block hash mismatch: Rust={rust_block['hash']} C#={csharp_block['hash']}"

    if rust_block["tx_hashes"] != csharp_block["tx_hashes"]:
        return False, "Transaction hashes mismatch"

    rust_root, r_err = get_stateroot(rust_url, height)
    if height in reference_roots:
        csharp_root = reference_roots[height]
        c_err = None
    else:
        csharp_root, c_err = get_stateroot(csharp_url, height)

    if r_err or c_err:
        return False, f"Stateroot fetch error: Rust={r_err} C#={c_err}"

    if rust_root != csharp_root:
        return False, f"Stateroot mismatch: Rust={rust_root} C#={csharp_root}"

    return True, None

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rust", default="http://127.0.0.1:20332")
    parser.add_argument("--csharp", default="http://seed1.neo.org:10332")
    parser.add_argument("--target", type=int, default=500000)
    args = parser.parse_args()

    reference_roots = {}
    stateroot_file = "data/reference_stateroots.jsonl"
    if os.path.exists(stateroot_file):
        with open(stateroot_file, "r") as f:
            for line in f:
                if line.strip():
                    try:
                        data = json.loads(line)
                        reference_roots[data["height"]] = data["roothash"]
                    except:
                        pass
    print(f"Loaded {len(reference_roots)} reference state roots locally.")

    print(f"Comparing up to {args.target} blocks between Rust ({args.rust}) and C# ({args.csharp})")

    state_file = ".compare_state"
    last_checked = -1

    # We saw in the previous log that it made it to 13449. So we can just resume from 13449.
    if os.path.exists(state_file):
        with open(state_file, "r") as f:
            try:
                last_checked = int(f.read().strip())
                print(f"Resuming from block {last_checked}...")
            except ValueError:
                pass
    else:
        last_checked = 13449
        print(f"Resuming from block {last_checked}...")

    batch_size = 20  # reduced batch size to minimize 502s from reference node

    while last_checked < args.target - 1:
        count, err = rpc_call(args.rust, "getblockcount", [])
        if not count:
            print("Waiting for Rust node RPC...", flush=True)
            time.sleep(5)
            continue

        current_height = count - 1

        if current_height <= last_checked:
            time.sleep(2)
            continue

        end_batch = min(last_checked + batch_size, current_height, args.target - 1)
        if end_batch <= last_checked:
            continue

        futures = {}
        # reduce concurrency to ease load on seed1.neo.org
        with ThreadPoolExecutor(max_workers=5) as executor:
            for h in range(last_checked + 1, end_batch + 1):
                futures[executor.submit(compare_height, args.rust, args.csharp, h, reference_roots)] = h

            for fut in as_completed(futures):
                h = futures[fut]
                ok, msg = fut.result()
                if not ok:
                    print(f"\nMismatch at height {h}: {msg}")
                    return 1

        last_checked = end_batch
        with open(state_file, "w") as f:
            f.write(str(last_checked))

        print(f"\rValidated up to {last_checked} blocks successfully (Local node height: {current_height})", end="", flush=True)

    print("\nAll 500K blocks validated successfully!")
    return 0

if __name__ == "__main__":
    exit(main())