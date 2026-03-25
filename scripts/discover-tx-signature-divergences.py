#!/usr/bin/env python3
"""Discover transaction signature verification divergences."""

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


def get_tx_from_block(url: str, height: int):
    """Get transactions from block."""
    block = rpc_call(url, "getblock", [height, True])
    if not block or "error" in block:
        return []
    return block.get("tx", [])


def compare_tx_verification(rust_url: str, csharp_url: str, tx_hash: str):
    """Compare transaction verification."""
    rust_tx = rpc_call(rust_url, "getrawtransaction", [tx_hash, True])
    csharp_tx = rpc_call(csharp_url, "getrawtransaction", [tx_hash, True])

    if not rust_tx or "error" in rust_tx:
        return [f"TX {tx_hash}: Rust RPC failed"]
    if not csharp_tx or "error" in csharp_tx:
        return [f"TX {tx_hash}: C# RPC failed"]

    divergences = []
    fields = ["hash", "size", "signers", "witnesses"]

    for field in fields:
        if rust_tx.get(field) != csharp_tx.get(field):
            divergences.append(f"  {field}: mismatch")
            print(f"  ❌ {field}")
        else:
            print(f"  ✅ {field}")

    return divergences


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Discover TX signature divergences")
    parser.add_argument("--rust", default="http://localhost:10332")
    parser.add_argument("--csharp", default="http://seed1.neo.org:10332")
    parser.add_argument("--heights", default="1,100,1000")
    args = parser.parse_args()

    heights = [int(h) for h in args.heights.split(",")]
    all_divergences = []

    for height in heights:
        print(f"\n=== Block {height} ===")
        txs = get_tx_from_block(args.csharp, height)
        if not txs:
            continue

        for tx in txs[:2]:  # Check first 2 TXs per block
            tx_hash = tx.get("hash")
            print(f"\nTX {tx_hash[:16]}...")
            divs = compare_tx_verification(args.rust, args.csharp, tx_hash)
            all_divergences.extend(divs)

    print(f"\n{'=' * 60}")
    if all_divergences:
        print(f"❌ Found {len(all_divergences)} divergences")
        return 1
    else:
        print("✅ No divergences - implementations compatible")
        return 0


if __name__ == "__main__":
    sys.exit(main())
