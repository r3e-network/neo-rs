#!/usr/bin/env python3
"""Verify neo-config protocol presets against live Neo N3 nodes.

Compares every consensus-relevant field in ``neo-config/src/protocol.rs``
(``mainnet()`` / ``testnet()``) with what a live C# node reports over
``getversion``. Any mismatch here means the Rust node would fork from the
network, so this check must stay green across releases.

Usage:
    python scripts/verify-protocol-presets.py [--mainnet-url URL] [--testnet-url URL]

Exit code 0 when every field matches, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
PROTOCOL_RS = REPO_ROOT / "neo-config" / "src" / "protocol.rs"

DEFAULT_MAINNET = "http://seed1.neo.org:10332"
DEFAULT_TESTNET = "http://seed1t5.neo.org:20332"

HF_ORDER = [
    "aspidochelone",
    "basilisk",
    "cockatrice",
    "domovoi",
    "echidna",
    "faun",
    "gorgon",
    "huyao",
]

FIELD_MAP = {
    "network": "network",
    "address_version": "addressversion",
    "ms_per_block": "msperblock",
    "max_valid_until_block_increment": "maxvaliduntilblockincrement",
    "validators_count": "validatorscount",
    "max_transactions_per_block": "maxtransactionsperblock",
    "memory_pool_max_transactions": "memorypoolmaxtransactions",
    "max_traceable_blocks": "maxtraceableblocks",
    "initial_gas_distribution": "initialgasdistribution",
}


def rpc(url: str, method: str, params: list | None = None) -> dict:
    payload = json.dumps(
        {"jsonrpc": "2.0", "method": method, "params": params or [], "id": 1}
    )
    out = subprocess.run(
        [
            "curl", "-s", "-m", "30", "-X", "POST", url,
            "-H", "Content-Type: application/json", "-d", payload,
        ],
        capture_output=True,
        text=True,
        timeout=60,
    ).stdout
    return json.loads(out)["result"]


def parse_preset(kind: str) -> dict:
    src = PROTOCOL_RS.read_text(encoding="utf-8")
    m = re.search(r"pub fn %s\(\) -> Self \{(.*?)\n    \}" % kind, src, re.S)
    if not m:
        raise SystemExit(f"cannot locate {kind}() in {PROTOCOL_RS}")
    body = m.group(1)
    nums: dict[str, int | None] = {}
    for name in FIELD_MAP:
        mm = re.search(r"%s: ([\d_]+)" % name, body)
        if mm:
            nums[name] = int(mm.group(1).replace("_", ""))
    av = re.search(r"address_version: (0x[0-9a-fA-F]+|\d+)", body)
    if av:
        nums["address_version"] = int(av.group(1), 0)
    return {
        "nums": nums,
        "keys": re.findall(r'"([0-9a-f]{66})"\.to_string\(\)', body),
        "hf": dict(re.findall(r"hf_(\w+): (Some\(\d+\)|None)", body)),
        "seeds": re.findall(r'"(seed[^"]*:\d+)"\.to_string\(\)', body),
    }


def check(label: str, url: str, kind: str) -> bool:
    live = rpc(url, "getversion")["protocol"]
    rust = parse_preset(kind)
    ok = True

    print("\n" + "=" * 64)
    print(f"{label}   (neo-config::{kind}()   vs   {url})")
    print("=" * 64)

    for rust_key, live_key in FIELD_MAP.items():
        r, l = rust["nums"].get(rust_key), live.get(live_key)
        good = r == l
        ok &= good
        print(f"  [{'OK      ' if good else 'MISMATCH'}] {rust_key:34s} rust={r}  live={l}")

    rk, lk = rust["keys"], live["standbycommittee"]
    if rk == lk:
        print(f"  [OK      ] {'standbycommittee':34s} {len(lk)} keys identical")
    else:
        ok = False
        print(f"  [MISMATCH] {'standbycommittee':34s} rust={len(rk)}  live={len(lk)}")
        for i, (a, b) in enumerate(zip(rk, lk)):
            if a != b:
                print(f"        idx {i}: rust={a}")
                print(f"                live={b}")

    live_hf = {h["name"].lower(): h["blockheight"] for h in live["hardforks"]}
    for h in HF_ORDER:
        rv = rust["hf"].get(h, "None")
        rv_int = int(rv[5:-1]) if rv.startswith("Some(") else None
        lv = live_hf.get(h)
        good = rv_int == lv
        ok &= good
        print(f"  [{'OK      ' if good else 'MISMATCH'}] hf_{h:18s} rust={rv_int}  live={lv}")

    extra = set(live_hf) - set(HF_ORDER)
    if extra:
        print(f"  [??      ] live hardforks unknown to rust: {sorted(extra)}")

    if rust["seeds"] == live.get("seedlist"):
        print(f"  [OK      ] {'seedlist':34s} identical")
    else:
        print(f"  [??      ] seedlist rust={rust['seeds']} live={live.get('seedlist')}")

    return ok


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mainnet-url", default=DEFAULT_MAINNET)
    ap.add_argument("--testnet-url", default=DEFAULT_TESTNET)
    args = ap.parse_args()

    results = [
        check("MAINNET", args.mainnet_url, "mainnet"),
        check("TESTNET T5", args.testnet_url, "testnet"),
    ]
    print("\n" + ("ALL MATCH" if all(results) else "*** DIVERGENCES FOUND ***"))
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
