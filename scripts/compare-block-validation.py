#!/usr/bin/env python3
"""Compare block validation between C# and Rust implementations."""

import argparse
import json
import urllib.request


def rpc_call(url: str, method: str, params: list):
    """Make JSON-RPC call."""
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode("utf-8")
    req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=20) as resp:
        result = json.loads(resp.read().decode("utf-8"))
    return result.get("result") if "result" in result else result


def get_block_details(url: str, height: int):
    """Get detailed block information for validation comparison."""
    block = rpc_call(url, "getblock", [height, True])
    if not block:
        return None

    return {
        "hash": block.get("hash"),
        "size": block.get("size"),
        "version": block.get("version"),
        "previousblockhash": block.get("previousblockhash"),
        "merkleroot": block.get("merkleroot"),
        "time": block.get("time"),
        "nonce": block.get("nonce"),
        "index": block.get("index"),
        "primary": block.get("primary"),
        "nextconsensus": block.get("nextconsensus"),
        "witnesses": block.get("witnesses"),
        "tx_count": len(block.get("tx", [])),
        "confirmations": block.get("confirmations"),
    }


def compare_blocks(rust_url: str, csharp_url: str, heights: list):
    """Compare block validation fields between implementations."""
    failures = []

    for height in heights:
        print(f"\nChecking block {height}...")
        rust_block = get_block_details(rust_url, height)
        csharp_block = get_block_details(csharp_url, height)

        if not rust_block or not csharp_block:
            failures.append(f"Height {height}: Failed to fetch block")
            continue

        # Compare critical validation fields
        for field in ["hash", "size", "merkleroot", "time", "tx_count", "witnesses"]:
            if rust_block.get(field) != csharp_block.get(field):
                failures.append(
                    f"Height {height} field '{field}': Rust={rust_block.get(field)} != C#={csharp_block.get(field)}"
                )
                print(f"  FAIL: {field}")
            else:
                print(f"  OK: {field}")

    return failures


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Compare block validation")
    parser.add_argument("--rust", required=True, help="Rust RPC URL")
    parser.add_argument("--csharp", required=True, help="C# RPC URL")
    parser.add_argument("--heights", default="0,1,100,1000", help="Comma-separated heights")
    args = parser.parse_args()

    heights = [int(h) for h in args.heights.split(",")]
    failures = compare_blocks(args.rust, args.csharp, heights)

    if failures:
        print(f"\n❌ Found {len(failures)} validation differences:")
        for f in failures:
            print(f"  - {f}")
        return 1
    else:
        print("\n✅ All block validation checks passed")
        return 0


if __name__ == "__main__":
    exit(main())
