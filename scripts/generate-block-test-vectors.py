#!/usr/bin/env python3
"""Generate block validation test vectors from C# node."""

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


def generate_vector(url: str, height: int):
    """Generate test vector for a block."""
    block_hex = rpc_call(url, "getblock", [height, False])
    block_json = rpc_call(url, "getblock", [height, True])

    return {
        "height": height,
        "block_hex": block_hex,
        "hash": block_json.get("hash"),
        "size": block_json.get("size"),
        "merkleroot": block_json.get("merkleroot"),
        "time": block_json.get("time"),
        "tx_count": len(block_json.get("tx", [])),
    }


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(description="Generate block test vectors")
    parser.add_argument("--rpc", required=True, help="C# node RPC URL")
    parser.add_argument("--heights", default="0,1,100,1000", help="Heights")
    parser.add_argument("--output", default="block_vectors.json", help="Output file")
    args = parser.parse_args()

    heights = [int(h) for h in args.heights.split(",")]
    vectors = [generate_vector(args.rpc, h) for h in heights]

    with open(args.output, "w") as f:
        json.dump(vectors, f, indent=2)

    print(f"Generated {len(vectors)} test vectors -> {args.output}")


if __name__ == "__main__":
    main()
