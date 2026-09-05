#!/usr/bin/env python3
"""Generate real MainNet block test vectors from a live Neo N3 node.

The repository previously shipped placeholder vectors (an all-zero
``mainnet_block_1000.hex`` and an empty ``mainnet_block_vectors()``), which
meant block serialisation/hashing was never checked against real chain data.
This script pulls genuine blocks over JSON-RPC and writes them to

    neo-core/tests/protocol_compliance/test_vectors/mainnet_blocks.json

Each vector carries the exact bytes the C# node produced, plus the fields a
Rust implementation must reproduce: hash, size, merkle root, timestamp and
transaction count.

Note: ``getblock <height> 0`` returns the block **base64**-encoded, not hex.
Decoding is mandatory - the earlier version of this script stored the base64
payload directly, which is why the checked-in vectors stayed empty.

Usage:
    python scripts/generate-block-test-vectors.py [--url URL] [--out PATH]
"""

from __future__ import annotations

import argparse
import base64
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_URL = "http://seed1.neo.org:10332"
DEFAULT_OUT = (
    REPO_ROOT
    / "neo-core"
    / "tests"
    / "protocol_compliance"
    / "test_vectors"
    / "mainnet_blocks.json"
)

# Hardfork activation heights on MainNet (verified via getversion, Neo 3.10.1).
HARDFORKS = {
    "Aspidochelone": 1_730_000,
    "Basilisk": 4_120_000,
    "Cockatrice": 5_450_000,
    "Domovoi": 5_570_000,
    "Echidna": 7_300_000,
    "Faun": 8_800_000,
    "Gorgon": 12_020_000,
}

# Hard cap so a pathological block cannot bloat the checked-in fixture.
# Large enough to admit the 512-transaction block below, which is the only
# MainNet block we found that exercises a deep (9-level) merkle tree.
MAX_BLOCK_BYTES = 160 * 1024

# Curated MainNet blocks that carry transactions, grouped by the hardfork era
# they belong to. Empty blocks have an all-zero merkle root, so without these
# merkle-root computation would go essentially untested.
# Discovered by sampling the chain with `getblock <h> 1` across full history.
TX_BEARING_BLOCKS = [
    (1_518_296, "pre-Aspidochelone, 2 transactions"),
    (1_693_472, "pre-Aspidochelone, 3 transactions"),
    (2_335_784, "Aspidochelone era, 2 transactions"),
    (3_970_760, "Aspidochelone era, 2 transactions"),
    (4_554_680, "Basilisk era, 512 transactions - deep merkle tree"),
    (6_131_264, "Echidna era, 3 transactions"),
    (6_773_576, "Echidna era, 3 transactions"),
    (8_583_728, "Echidna era, 2 transactions"),
    (10_744_232, "Faun era, 1 large transaction"),
    (12_145_640, "Gorgon era, 1 transaction"),
]


def rpc(url: str, method: str, params: list):
    """Make a JSON-RPC call and return `result`."""
    payload = json.dumps({"jsonrpc": "2.0", "method": method, "params": params, "id": 1})
    out = subprocess.run(
        [
            "curl", "-s", "-m", "45", "-X", "POST", url,
            "-H", "Content-Type: application/json", "-d", payload,
        ],
        capture_output=True,
        text=True,
        timeout=90,
    ).stdout
    doc = json.loads(out)
    if "error" in doc:
        raise RuntimeError(f"RPC {method}{params} failed: {doc['error']}")
    return doc["result"]


def fetch_block(url: str, height: int):
    """Return a vector for `height`, or None if it is unusable."""
    raw = rpc(url, "getblock", [height, 0])  # base64-encoded block bytes
    data = base64.b64decode(raw)
    if len(data) > MAX_BLOCK_BYTES:
        print(f"  skip {height}: {len(data)} bytes exceeds cap")
        return None
    meta = rpc(url, "getblock", [height, 1])
    tx = meta.get("tx", [])
    return {
        "height": height,
        "block_hex": data.hex(),
        "hash": meta["hash"],
        "size": meta["size"],
        "merkleroot": meta["merkleroot"],
        "time": meta["time"],
        "tx_count": len(tx) if isinstance(tx, list) else 0,
        "previousblockhash": meta.get("previousblockhash", ""),
        "nonce": meta.get("nonce", ""),
        "primary": meta.get("primary", 0),
        "nextconsensus": meta.get("nextconsensus", ""),
        "note": "",
    }


def heights_around_hardforks() -> list[tuple[int, str]]:
    plan: list[tuple[int, str]] = [
        (0, "genesis"),
        (1, "first block after genesis"),
    ]
    for name, h in HARDFORKS.items():
        plan.append((h - 1, f"last block before {name}"))
        plan.append((h, f"{name} activation"))
    return plan


def find_blocks_with_txs(url: str, start: int, want: int = 4, scan: int = 500):
    """Scan backwards for blocks that actually carry transactions.

    Empty blocks have an all-zero merkle root, which would leave merkle-root
    computation completely untested.
    """
    found = []
    height = start
    while height > start - scan and len(found) < want:
        head = rpc(url, "getblockheader", [height, 1])
        tx = head.get("tx", 0)
        tx_count = len(tx) if isinstance(tx, list) else (tx if isinstance(tx, int) else 0)
        if tx_count > 0:
            found.append((height, f"block with {tx_count} transactions"))
        height -= 1
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate block test vectors")
    parser.add_argument("--url", default=DEFAULT_URL, help="C# node RPC URL")
    parser.add_argument("--out", default=str(DEFAULT_OUT), help="Output file")
    args = parser.parse_args()

    version = rpc(args.url, "getversion", [])
    net = version["protocol"]["network"]
    agent = version["useragent"]
    tip = rpc(args.url, "getblockcount", []) - 1
    print(f"source    : {args.url}  ({agent})")
    print(f"network   : {net}")
    print(f"chain tip : {tip}")

    plan = heights_around_hardforks()
    # Hardfork-boundary blocks are almost always empty, so merkle-root coverage
    # has to come from an explicit curated set.
    plan += TX_BEARING_BLOCKS
    plan.append((tip, "chain tip"))

    vectors = []
    seen = set()
    print("\nfetching blocks:")
    for height, note in plan:
        if height in seen or height < 0 or height > tip:
            continue
        seen.add(height)
        try:
            vec = fetch_block(args.url, height)
        except Exception as exc:  # noqa: BLE001 - report and keep going
            print(f"  {height:>9}: FAILED ({exc})")
            continue
        if vec is None:
            continue
        vec["note"] = note
        vectors.append(vec)
        print(
            f"  {height:>9}: size={vec['size']:<6} tx={vec['tx_count']:<4} "
            f"hash={vec['hash'][:18]}...  {note}"
        )

    vectors.sort(key=lambda v: v["height"])
    payload = {
        "network": "mainnet",
        "magic": net,
        "source": f"{args.url} ({agent})",
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "chain_tip": tip,
        "block_count": len(vectors),
        "blocks": vectors,
    }

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    total = sum(len(v["block_hex"]) // 2 for v in vectors)
    print(f"\nwrote {len(vectors)} vectors ({total:,} bytes of block data)")
    print(f"  -> {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
