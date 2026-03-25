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
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            "Accept-Encoding": "identity",
            "User-Agent": "neo-rs-sync-samples/1.0",
        },
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


def normalize_blockish(value):
    if isinstance(value, dict):
        value = dict(value)
        value.pop("confirmations", None)
        value.pop("nextblockhash", None)
        value.pop("blocktime", None)
        return {k: normalize_blockish(v) for k, v in value.items()}
    if isinstance(value, list):
        return [normalize_blockish(item) for item in value]
    return value


def build_default_heights(max_height: int):
    seed = [0, 1, 2, 10, 50, 100, 500, 1000, 5000, 10000, 15000, 18000]
    heights = {height for height in seed if height <= max_height}

    if max_height > 0:
        step = max(max_height // 10, 1)
        current = step
        while current < max_height:
            heights.add(current)
            current += step
        heights.add(max_height)

    return sorted(heights)


def parse_candidate_heights(raw: str, max_height: int):
    values = set()
    for part in raw.split(","):
        part = part.strip()
        if not part:
            continue
        value = int(part)
        if 0 <= value <= max_height:
            values.add(value)
    return sorted(values)


def compare(rust_rpc: str, csharp_rpc: str, candidate_heights_raw: str | None):
    rust_count = rpc_call(rust_rpc, "getblockcount", [])
    csharp_count = rpc_call(csharp_rpc, "getblockcount", [])
    max_height = min(rust_count, csharp_count) - 1

    if max_height < 0:
        print("FAIL no common synced height available")
        return 1

    if candidate_heights_raw:
        heights = parse_candidate_heights(candidate_heights_raw, max_height)
    else:
        heights = build_default_heights(max_height)

    failures = []

    for height in heights:
        rust_hash = rpc_call(rust_rpc, "getblockhash", [height])
        csharp_hash = rpc_call(csharp_rpc, "getblockhash", [height])
        if rust_hash != csharp_hash:
            failures.append((f"getblockhash({height})", rust_hash, csharp_hash))
            break
        print(f"OK   getblockhash({height})")

        rust_block = normalize_blockish(rpc_call(rust_rpc, "getblock", [height, True]))
        csharp_block = normalize_blockish(rpc_call(csharp_rpc, "getblock", [height, True]))
        if rust_block != csharp_block:
            failures.append((f"getblock({height}, true)", rust_block, csharp_block))
            break
        print(f"OK   getblock({height}, true)")

        rust_header = normalize_blockish(rpc_call(rust_rpc, "getblockheader", [height, True]))
        csharp_header = normalize_blockish(rpc_call(csharp_rpc, "getblockheader", [height, True]))
        if rust_header != csharp_header:
            failures.append((f"getblockheader({height}, true)", rust_header, csharp_header))
            break
        print(f"OK   getblockheader({height}, true)")

    # Compare a couple of historical transactions from the sampled prefix if available.
    for height in heights:
        block = rpc_call(rust_rpc, "getblock", [height, True])
        txs = block.get("tx", [])
        if not txs:
            continue
        txid = txs[0]["hash"]
        rust_tx = normalize_blockish(rpc_call(rust_rpc, "getrawtransaction", [txid, True]))
        csharp_tx = normalize_blockish(rpc_call(csharp_rpc, "getrawtransaction", [txid, True]))
        if rust_tx != csharp_tx:
            failures.append((f"getrawtransaction({txid}, true)", rust_tx, csharp_tx))
            break
        print(f"OK   getrawtransaction({txid}, true)")

        rust_height = rpc_call(rust_rpc, "gettransactionheight", [txid])
        csharp_height = rpc_call(csharp_rpc, "gettransactionheight", [txid])
        if rust_height != csharp_height:
            failures.append((f"gettransactionheight({txid})", rust_height, csharp_height))
            break
        print(f"OK   gettransactionheight({txid})")
        # One transaction sample is enough for this lightweight check.
        break

    if failures:
        print("")
        label, rust_value, csharp_value = failures[0]
        print(f"FAIL {label}")
        print("rust:")
        print(json.dumps(rust_value, indent=2, sort_keys=True))
        print("csharp:")
        print(json.dumps(csharp_value, indent=2, sort_keys=True))
        return 1

    print("")
    print("All sampled synced-state checks matched.")
    print(f"rust blockcount:   {rust_count}")
    print(f"csharp blockcount: {csharp_count}")
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Compare sampled synced-state RPC results between local Rust and C# Neo nodes."
    )
    parser.add_argument("--rust-rpc", required=True)
    parser.add_argument("--csharp-rpc", required=True)
    parser.add_argument(
        "--candidate-heights",
        help="Comma-separated block heights to sample instead of the built-in distribution",
    )
    args = parser.parse_args()
    sys.exit(compare(args.rust_rpc, args.csharp_rpc, args.candidate_heights))


if __name__ == "__main__":
    main()
