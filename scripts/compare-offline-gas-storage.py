#!/usr/bin/env python3
"""Compare offline neo-rs GAS account storage against a reference state root."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any, Callable


SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from neo_storage_tools import (  # noqa: E402
    DEFAULT_REFERENCE_RPC,
    GAS_HASH,
    decode_nep17_account_balance,
    gas_account_storage_key,
    rpc_call,
)


ProbeRunner = Callable[[Path, list[str]], dict]
RpcCaller = Callable[[str, str, list, float], Any]
def display_hash_from_le(hash_hex_le: str) -> str:
    return "0x" + bytes.fromhex(hash_hex_le)[::-1].hex()


def run_probe(
    db_path: Path,
    args: list[str],
    *,
    probe_bin: Path,
) -> dict:
    command = [
        str(probe_bin),
        "--db",
        str(db_path),
        *args,
    ]
    completed = subprocess.run(
        command,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return json.loads(completed.stdout)


def local_ledger_pointer(
    *,
    db_path: Path,
    probe_bin: Path,
    probe_runner: Callable[..., dict],
) -> dict:
    payload = probe_runner(
        db_path,
        ["--contract-id", "-4", "--key-hex", "0c", "--decode", "hash-index"],
        probe_bin=probe_bin,
    )
    decoded = payload.get("decoded") or {}
    if not payload.get("found") or decoded.get("format") != "hash-index":
        raise ValueError(f"Ledger current-block pointer not found in {db_path}")
    return {
        "height": int(decoded["index"]),
        "hash": display_hash_from_le(decoded["hash_hex_le"]),
    }


def local_gas_balance(
    *,
    db_path: Path,
    address: str,
    probe_bin: Path,
    probe_runner: Callable[..., dict],
) -> dict:
    payload = probe_runner(
        db_path,
        ["--gas-address", address, "--decode", "nep17-account"],
        probe_bin=probe_bin,
    )
    decoded = payload.get("decoded") or {}
    if payload.get("found"):
        balance = int(decoded["balance"])
    else:
        balance = 0
    return {
        "found": bool(payload.get("found")),
        "key_base64": payload.get("key_base64") or gas_account_storage_key(address),
        "balance": balance,
    }


def compare_gas_accounts(
    *,
    db_path: Path,
    addresses: list[str],
    probe_bin: Path,
    reference_rpc: str,
    probe_runner: Callable[..., dict] = run_probe,
    rpc: RpcCaller = rpc_call,
) -> dict:
    ledger = local_ledger_pointer(
        db_path=db_path,
        probe_bin=probe_bin,
        probe_runner=probe_runner,
    )
    height = ledger["height"]
    reference_block = rpc(reference_rpc, "getblock", [height, 1], 20.0)
    reference_block_hash = reference_block.get("hash")
    root_hash = rpc(reference_rpc, "getstateroot", [height], 20.0)["roothash"]

    balances = []
    for address in addresses:
        local = local_gas_balance(
            db_path=db_path,
            address=address,
            probe_bin=probe_bin,
            probe_runner=probe_runner,
        )
        reference_value = rpc(
            reference_rpc,
            "getstate",
            [root_hash, GAS_HASH, local["key_base64"]],
            20.0,
        )
        reference_balance = decode_nep17_account_balance(reference_value)
        delta = local["balance"] - reference_balance
        balances.append(
            {
                "address": address,
                "gas_account_key_base64": local["key_base64"],
                "local_found": local["found"],
                "local_balance": local["balance"],
                "reference_balance": reference_balance,
                "delta": delta,
                "matches": delta == 0,
            }
        )

    canonical_block_hash_match = ledger["hash"] == reference_block_hash
    all_balances_match = all(item["matches"] for item in balances)
    return {
        "db": str(db_path),
        "storage_provider": "mdbx",
        "height": height,
        "local_block_hash": ledger["hash"],
        "reference_block_hash": reference_block_hash,
        "canonical_block_hash_match": canonical_block_hash_match,
        "reference_state_root": root_hash,
        "all_balances_match": all_balances_match,
        "balances": balances,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Compare offline GAS account storage from a neo-rs MDBX directory "
            "against Neo reference state at the DB's current Ledger height."
        )
    )
    parser.add_argument("--db", required=True, type=Path, help="neo-rs chain MDBX path")
    parser.add_argument(
        "--address",
        action="append",
        required=True,
        help="Neo address whose GAS AccountState should match the reference root; repeatable",
    )
    parser.add_argument(
        "--probe-bin",
        default=Path("target/debug/neo-db-probe"),
        type=Path,
        help="Path to the built neo-db-probe binary",
    )
    parser.add_argument(
        "--reference-rpc",
        default=DEFAULT_REFERENCE_RPC,
        help=f"Reference Neo RPC endpoint (default: {DEFAULT_REFERENCE_RPC})",
    )
    parser.add_argument(
        "--allow-mismatch",
        action="store_true",
        help="Exit 0 even when the local block hash or sampled balances diverge.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    report = compare_gas_accounts(
        db_path=args.db,
        addresses=args.address,
        probe_bin=args.probe_bin,
        reference_rpc=args.reference_rpc,
    )
    print(json.dumps(report, indent=2, sort_keys=True))
    if args.allow_mismatch:
        return 0
    if report["canonical_block_hash_match"] and report["all_balances_match"]:
        return 0
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
