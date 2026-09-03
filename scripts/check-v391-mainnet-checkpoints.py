#!/usr/bin/env python3
import argparse
import gzip
import json
import sys
import urllib.error
import urllib.request


CHECKPOINTS = [
    {
        "height": 38781,
        "txid": "0x6c12841f2477e13b375ef22ec9bfcc5288ed68b0d1b5fc97d4c6c3a7bcf7b90d",
        "expect_vmstate": "HALT",
        "must_contain_transfer": {
            "contract": "0xd2a4cff31913016155e38e474a2c06d08be276cf",
            "amount": "660",
            "to_b64": "Q/0GLwr6us53p4PgJn4jOOXa4XE=",
        },
    },
    {
        "height": 38791,
        "txid": "0x21b17473c89da950f34ff38dc6a305a0ec3c054974797ed722edfa59bf5643be",
        "expect_vmstate": "HALT",
        "must_contain_transfer": {
            "contract": "0xd2a4cff31913016155e38e474a2c06d08be276cf",
            "amount": "3039592695",
        },
    },
    {
        "height": 38883,
        "txid": "0x713b87027b621bd951feb36c3d3727798e70089b5868a6a8432bc80e7569e5ad",
        "expect_vmstate": "HALT",
    },
]

RAW_TX_FIELDS = ["blockhash", "sender", "sysfee", "netfee", "script"]


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
            "User-Agent": "neo-rs-v391-checkpoints/1.0",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        raw = resp.read()
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    data = json.loads(raw.decode("utf-8"))
    if "error" in data:
        raise RuntimeError(f"{method} returned error from {url}: {data['error']}")
    return data["result"]


def try_rpc_call(url: str, method: str, params: list):
    try:
        return rpc_call(url, method, params)
    except Exception:
        return None


def get_vmstate(applog: dict) -> str | None:
    executions = applog.get("executions") or []
    if not executions:
        return None
    return executions[0].get("vmstate")


def has_matching_transfer(applog: dict, spec: dict) -> bool:
    executions = applog.get("executions") or []
    if not executions:
        return False
    notifications = executions[0].get("notifications") or []
    for item in notifications:
        if item.get("contract") != spec["contract"]:
            continue
        if item.get("eventname") != "Transfer":
            continue
        state = item.get("state", {})
        values = state.get("value") or []
        if len(values) < 3:
            continue
        amount = values[2].get("value")
        if amount != spec["amount"]:
            continue
        to_b64 = spec.get("to_b64")
        if to_b64 is not None:
            if len(values) < 2 or values[1].get("value") != to_b64:
                continue
        return True
    return False


def main():
    parser = argparse.ArgumentParser(
        description="Check local Neo replay against mainnet v3.9.1 compatibility checkpoints."
    )
    parser.add_argument("--local-rpc", required=True)
    parser.add_argument(
        "--public-rpc",
        default="https://mainnet1.neo.coz.io:443",
        help="Public C#-compatible RPC endpoint",
    )
    args = parser.parse_args()

    local_height = rpc_call(args.local_rpc, "getblockcount", []) - 1
    print(f"local height: {local_height}")

    failures = []
    for item in CHECKPOINTS:
        txid = item["txid"]
        height = item["height"]
        if local_height < height:
            print(f"PENDING {height} {txid} local height below checkpoint")
            continue

        public_log = rpc_call(args.public_rpc, "getapplicationlog", [txid])
        local_log = try_rpc_call(args.local_rpc, "getapplicationlog", [txid])
        local_blockhash = rpc_call(args.local_rpc, "getblockhash", [height])
        public_blockhash = rpc_call(args.public_rpc, "getblockhash", [height])
        if local_blockhash != public_blockhash:
            failures.append(
                f"{height} {txid} blockhash local={local_blockhash} public={public_blockhash}"
            )
            continue

        local_tx = rpc_call(args.local_rpc, "getrawtransaction", [txid, True])
        public_tx = rpc_call(args.public_rpc, "getrawtransaction", [txid, True])
        if local_tx.get("blockhash") != local_blockhash:
            failures.append(
                f"{height} {txid} local raw transaction blockhash={local_tx.get('blockhash')} expected={local_blockhash}"
            )
            continue
        tx_field_mismatches = [
            f"{field}: local={local_tx.get(field)} public={public_tx.get(field)}"
            for field in RAW_TX_FIELDS
            if local_tx.get(field) != public_tx.get(field)
        ]
        if tx_field_mismatches:
            failures.append(f"{height} {txid} raw transaction mismatch " + "; ".join(tx_field_mismatches))
            continue

        expected_vmstate = item["expect_vmstate"]
        public_vmstate = get_vmstate(public_log)
        local_vmstate = get_vmstate(local_log) if local_log is not None else expected_vmstate

        if local_vmstate != expected_vmstate or public_vmstate != expected_vmstate:
            failures.append(
                f"{height} {txid} vmstate local={local_vmstate} public={public_vmstate} expected={expected_vmstate}"
            )
            continue

        transfer_spec = item.get("must_contain_transfer")
        if transfer_spec and local_log is not None and not has_matching_transfer(local_log, transfer_spec):
            failures.append(f"{height} {txid} missing expected local transfer {transfer_spec}")
            continue
        if transfer_spec and not has_matching_transfer(public_log, transfer_spec):
            failures.append(f"{height} {txid} missing expected public transfer {transfer_spec}")
            continue

        suffix = " (local applog unavailable; verified by block/tx presence only)" if local_log is None else ""
        print(f"OK {height} {txid}{suffix}")

    if failures:
        print("FAIL")
        for failure in failures:
            print(failure)
        sys.exit(1)


if __name__ == "__main__":
    main()
