#!/usr/bin/env python3
import argparse
import gzip
import json
import sys
import urllib.request


def rpc_call(url: str, method: str, params: list):
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    ).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=payload,
        headers={"Content-Type": "application/json", "Accept-Encoding": "identity"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        raw = resp.read()
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    payload = json.loads(raw.decode("utf-8"))
    if "error" in payload:
        raise RuntimeError(f"{method} returned error from {url}: {payload['error']}")
    return payload["result"]


def compare(rust_rpc: str, csharp_rpc: str, max_common_height: int):
    rust_count = rpc_call(rust_rpc, "getblockcount", [])
    csharp_count = rpc_call(csharp_rpc, "getblockcount", [])
    common = min(rust_count, csharp_count) - 1
    common = min(common, max_common_height)

    if common < 0:
        print("FAIL neither node has synced even the genesis block")
        return 1

    print(f"rust blockcount:   {rust_count}")
    print(f"csharp blockcount: {csharp_count}")
    print(f"comparing common prefix through height {common}")

    failures = []
    for height in range(common + 1):
        rust_hash = rpc_call(rust_rpc, "getblockhash", [height])
        csharp_hash = rpc_call(csharp_rpc, "getblockhash", [height])
        if rust_hash != csharp_hash:
            failures.append((height, rust_hash, csharp_hash))
            print(f"FAIL height {height}")
            break

    if failures:
        height, rust_hash, csharp_hash = failures[0]
        print(f"rust blockhash[{height}]   = {rust_hash}")
        print(f"csharp blockhash[{height}] = {csharp_hash}")
        return 1

    print("OK   all compared block hashes matched")
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Compare the common synced blockhash window between local Rust and C# Neo nodes."
    )
    parser.add_argument("--rust-rpc", required=True)
    parser.add_argument("--csharp-rpc", required=True)
    parser.add_argument(
        "--max-common-height",
        type=int,
        default=200,
        help="Maximum height to compare in the common synced prefix",
    )
    args = parser.parse_args()
    sys.exit(compare(args.rust_rpc, args.csharp_rpc, args.max_common_height))


if __name__ == "__main__":
    main()
