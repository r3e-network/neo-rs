#!/usr/bin/env python3
"""Repair bounded replay GAS account drift from a reference state root.

This tool is intentionally scoped to validation/replay workspaces. It parses a
neo-node log for deterministic `GasToken::burn` balance failures, identifies the
reference block transaction whose `sysfee + netfee` equals the failed burn, then
copies that sender's GAS NEP-17 AccountState from the official state root at
`height - 1` into the local RocksDB via `neo-db-probe`.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Callable


SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from neo_storage_tools import (  # noqa: E402
    DEFAULT_ADDRESS_VERSION,
    DEFAULT_REFERENCE_RPC,
    GAS_HASH,
    decode_nep17_account_balance,
    gas_account_storage_key,
    rpc_call,
)


GAS_BURN_RE = re.compile(
    r"native GasToken TriggerType\(ON_PERSIST\) hook failed at block "
    r"(?P<height>\d+): .*?"
    r"GasToken::burn: insufficient balance (?P<balance>\d+) to burn (?P<burn>\d+)"
)

ProbeWriter = Callable[[Path, str, str, Path], dict]
LocalBalanceReader = Callable[[Path, str, Path], dict]
RpcCaller = Callable[[str, str, list, float], Any]


def parse_gas_burn_failures(text: str) -> list[dict]:
    failures = []
    seen = set()
    for match in GAS_BURN_RE.finditer(text):
        item = {
            "height": int(match.group("height")),
            "local_balance": int(match.group("balance")),
            "burn_amount": int(match.group("burn")),
        }
        key = (item["height"], item["local_balance"], item["burn_amount"])
        if key in seen:
            continue
        seen.add(key)
        failures.append(item)
    return failures


def int_field(value: Any) -> int:
    if isinstance(value, int):
        return value
    if isinstance(value, str):
        return int(value)
    raise ValueError(f"expected integer-like value, got {value!r}")


def matching_fee_transactions(reference_block: dict, burn_amount: int) -> list[dict]:
    matches = []
    sender_prior_fees: dict[str, int] = {}
    for index, tx in enumerate(reference_block.get("tx") or []):
        sysfee = int_field(tx.get("sysfee", 0))
        netfee = int_field(tx.get("netfee", 0))
        total_fee = sysfee + netfee
        sender = tx.get("sender")
        prior_fees = sender_prior_fees.get(sender, 0) if sender else 0
        if total_fee != burn_amount:
            if sender:
                sender_prior_fees[sender] = prior_fees + total_fee
            continue
        matches.append(
            {
                "index": index,
                "hash": tx.get("hash"),
                "sender": sender,
                "sysfee": sysfee,
                "netfee": netfee,
                "total_fee": total_fee,
                "sender_prior_fees": prior_fees,
            }
        )
        if sender:
            sender_prior_fees[sender] = prior_fees + total_fee
    return matches


def read_local_gas_balance(db_path: Path, sender: str, probe_bin: Path) -> dict:
    completed = subprocess.run(
        [
            str(probe_bin),
            "--db",
            str(db_path),
            "--gas-address",
            sender,
            "--decode",
            "nep17-account",
        ],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    payload = json.loads(completed.stdout)
    decoded = payload.get("decoded") or {}
    balance = int(decoded.get("balance", 0)) if payload.get("found") else 0
    return {
        "sender": sender,
        "found": bool(payload.get("found")),
        "balance": balance,
        "key_base64": payload.get("key_base64"),
    }


def choose_matching_transaction(
    *,
    matches: list[dict],
    failure: dict,
    db_path: Path | None,
    probe_bin: Path | None,
    local_balance_reader: LocalBalanceReader = read_local_gas_balance,
) -> tuple[dict, list[dict]]:
    if len(matches) == 1:
        return matches[0], []
    if not matches:
        raise ValueError(
            "no reference transaction has sysfee+netfee "
            f"{failure['burn_amount']} at height {failure['height']}"
        )
    if db_path is None or probe_bin is None:
        raise ValueError(
            "multiple reference transactions match the failed burn amount; "
            "provide a local DB/probe so sender balances can disambiguate"
        )
    candidates = []
    for tx in matches:
        sender = tx.get("sender")
        if not sender:
            continue
        local = local_balance_reader(db_path, sender, probe_bin)
        prior_fees = int(tx.get("sender_prior_fees", 0))
        projected_balance = local["balance"] - prior_fees
        candidates.append(
            {
                **tx,
                "local": local,
                "projected_local_balance": projected_balance,
            }
        )
    matching_local = [
        item for item in candidates if item["local"]["balance"] == failure["local_balance"]
    ]
    if len(matching_local) != 1:
        matching_projected = [
            item
            for item in candidates
            if item["projected_local_balance"] == failure["local_balance"]
        ]
        if len(matching_projected) == 1:
            return matching_projected[0], candidates
        unique_senders = {
            item.get("sender")
            for item in candidates
            if item.get("sender")
        }
        if len(unique_senders) == 1:
            return candidates[0], candidates
        raise ValueError(
            "expected exactly one fee-matching sender whose local GAS balance "
            f"is {failure['local_balance']}, got {len(matching_local)}"
        )
    return matching_local[0], candidates


def choose_failure(failures: list[dict], which: str) -> dict:
    if not failures:
        raise ValueError("no GasToken::burn balance failure found in log")
    if which == "first":
        return failures[0]
    if which == "latest":
        return failures[-1]
    raise ValueError(f"unsupported failure selector: {which}")


def read_log_text(log_path: Path, start_offset: int = 0) -> str:
    with log_path.open("rb") as handle:
        handle.seek(start_offset)
        return handle.read().decode("utf-8", errors="replace")


def build_repair_plan(
    *,
    log_path: Path,
    reference_rpc: str,
    address_version: int,
    which_failure: str,
    log_start_offset: int = 0,
    db_path: Path | None = None,
    probe_bin: Path | None = None,
    rpc: RpcCaller = rpc_call,
    local_balance_reader: LocalBalanceReader = read_local_gas_balance,
) -> dict:
    failures = parse_gas_burn_failures(read_log_text(log_path, log_start_offset))
    failure = choose_failure(failures, which_failure)
    reference_block = rpc(reference_rpc, "getblock", [failure["height"], 1], 20.0)
    matches = matching_fee_transactions(reference_block, failure["burn_amount"])
    tx, candidates = choose_matching_transaction(
        matches=matches,
        failure=failure,
        db_path=db_path,
        probe_bin=probe_bin,
        local_balance_reader=local_balance_reader,
    )
    sender = tx["sender"]
    if not sender:
        raise ValueError(f"matched transaction {tx.get('hash')} has no sender")
    state_root = rpc(reference_rpc, "getstateroot", [failure["height"] - 1], 20.0)["roothash"]
    key_base64 = gas_account_storage_key(sender, address_version=address_version)
    value_base64 = rpc(reference_rpc, "getstate", [state_root, GAS_HASH, key_base64], 20.0)
    return {
        "log": {
            "path": str(log_path),
            "start_offset": log_start_offset,
        },
        "failure": failure,
        "reference": {
            "block_hash": reference_block.get("hash"),
            "state_height": failure["height"] - 1,
            "state_root": state_root,
            "matched_transaction": tx,
            "candidate_transactions": candidates,
        },
        "repair": {
            "sender": sender,
            "gas_account_key_base64": key_base64,
            "value_base64": value_base64,
            "reference_balance": decode_nep17_account_balance(value_base64),
        },
    }


def write_gas_account(db_path: Path, sender: str, value_base64: str, probe_bin: Path) -> dict:
    completed = subprocess.run(
        [
            str(probe_bin),
            "--db",
            str(db_path),
            "--gas-address",
            sender,
            "--write-value-base64",
            value_base64,
        ],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return json.loads(completed.stdout)


def repair_bounded_replay_gas(
    *,
    db_path: Path,
    log_path: Path,
    probe_bin: Path,
    reference_rpc: str,
    address_version: int = DEFAULT_ADDRESS_VERSION,
    which_failure: str = "latest",
    log_start_offset: int = 0,
    apply: bool = False,
    rpc: RpcCaller = rpc_call,
    probe_writer: ProbeWriter = write_gas_account,
    local_balance_reader: LocalBalanceReader = read_local_gas_balance,
) -> dict:
    plan = build_repair_plan(
        log_path=log_path,
        reference_rpc=reference_rpc,
        address_version=address_version,
        which_failure=which_failure,
        log_start_offset=log_start_offset,
        db_path=db_path,
        probe_bin=probe_bin,
        rpc=rpc,
        local_balance_reader=local_balance_reader,
    )
    result = {
        "db": str(db_path),
        "log": str(log_path),
        "applied": False,
        **plan,
    }
    if apply:
        result["probe"] = probe_writer(
            db_path,
            plan["repair"]["sender"],
            plan["repair"]["value_base64"],
            probe_bin,
        )
        result["applied"] = True
    return result


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Repair a bounded replay RocksDB after a GasToken::burn balance failure "
            "by copying the sender GAS AccountState from the reference state root."
        )
    )
    parser.add_argument("--db", required=True, type=Path, help="bounded replay chain RocksDB path")
    parser.add_argument("--log", required=True, type=Path, help="neo-node log file to scan")
    parser.add_argument(
        "--probe-bin",
        default=Path("target/release/neo-db-probe"),
        type=Path,
        help="neo-db-probe binary used for the local write",
    )
    parser.add_argument(
        "--reference-rpc",
        default=DEFAULT_REFERENCE_RPC,
        help=f"Reference Neo RPC endpoint (default: {DEFAULT_REFERENCE_RPC})",
    )
    parser.add_argument(
        "--address-version",
        default=DEFAULT_ADDRESS_VERSION,
        type=lambda value: int(value, 0),
        help=f"Neo address version (default: 0x{DEFAULT_ADDRESS_VERSION:02x})",
    )
    parser.add_argument(
        "--failure",
        choices=["first", "latest"],
        default="latest",
        help="Which unique GasToken burn failure in the log to repair",
    )
    parser.add_argument(
        "--log-start-offset",
        default=0,
        type=int,
        help="Only scan log bytes at or after this offset.",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="Write the reference value to the local DB; without this, only print the plan",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        payload = repair_bounded_replay_gas(
            db_path=args.db,
            log_path=args.log,
            probe_bin=args.probe_bin,
            reference_rpc=args.reference_rpc,
            address_version=args.address_version,
            which_failure=args.failure,
            log_start_offset=args.log_start_offset,
            apply=args.apply,
        )
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
