#!/usr/bin/env python3
"""Discover block validation divergences between Rust and C# implementations."""

import argparse
import json
import sys
import urllib.request


def rpc_call(url: str, method: str, params: list):
    """Make JSON-RPC call."""
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            result = json.loads(resp.read().decode())
        return result.get("result")
    except Exception as e:
        return {"error": str(e)}


def compare_block(rust_url: str, csharp_url: str, height: int):
    """Compare block at given height."""
    print(f"\n=== Block {height} ===")

    rust_block = rpc_call(rust_url, "getblock", [height, True])
    csharp_block = rpc_call(csharp_url, "getblock", [height, True])

    if not rust_block or "error" in rust_block:
        return [f"Height {height}: Rust RPC failed"]
    if not csharp_block or "error" in csharp_block:
        return [f"Height {height}: C# RPC failed"]

    divergences = []
    fields = ["hash", "size", "merkleroot", "time", "nonce", "primary", "nextconsensus"]

    for field in fields:
        rust_val = rust_block.get(field)
        csharp_val = csharp_block.get(field)
        if rust_val != csharp_val:
            divergences.append(f"  {field}: Rust={rust_val} != C#={csharp_val}")
            print(f"  ❌ {field}")
        else:
            print(f"  ✅ {field}")

    return divergences


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Discover block validation divergences")
    parser.add_argument("--rust", default="http://localhost:10332", help="Rust node RPC")
    parser.add_argument("--csharp", default="http://seed1.neo.org:10332", help="C# node RPC")
    parser.add_argument("--heights", default="0,1,1000,10000", help="Heights to test")
    args = parser.parse_args()

    heights = [int(h) for h in args.heights.split(",")]
    all_divergences = []

    for height in heights:
        divs = compare_block(args.rust, args.csharp, height)
        all_divergences.extend(divs)

    print(f"\n{'=' * 60}")
    if all_divergences:
        print(f"❌ Found {len(all_divergences)} divergences:")
        for d in all_divergences:
            print(d)
        return 1
    else:
        print("✅ No divergences found - implementations are compatible")
        return 0


if __name__ == "__main__":
    sys.exit(main())
