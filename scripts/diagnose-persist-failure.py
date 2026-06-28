#!/usr/bin/env python3
"""Diagnose deterministic native persistence failures from neo-node logs."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from neo_storage_tools import (  # noqa: E402
    DEFAULT_ADDRESS_VERSION,
    DEFAULT_REFERENCE_RPC,
    decode_nep17_account_balance,
    decode_storage_integer,
    gas_account_storage_key,
    rpc_call,
)


GAS_BURN_RE = re.compile(
    r"native (?P<contract>GasToken) TriggerType\(ON_PERSIST\) hook failed at block "
    r"(?P<height>\d+): .*?"
    r"GasToken::burn: insufficient balance (?P<balance>\d+) to burn (?P<burn>\d+)"
)


def parse_gas_burn_failures(text: str) -> list[dict]:
    failures = []
    seen = set()
    for match in GAS_BURN_RE.finditer(text):
        item = {
            "height": int(match.group("height")),
            "balance": int(match.group("balance")),
            "burn_amount": int(match.group("burn")),
            "contract": match.group("contract"),
        }
        key = (item["height"], item["balance"], item["burn_amount"], item["contract"])
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
    for tx in reference_block.get("tx") or []:
        sysfee = int_field(tx.get("sysfee", 0))
        netfee = int_field(tx.get("netfee", 0))
        total_fee = sysfee + netfee
        if total_fee != burn_amount:
            continue
        matches.append(
            {
                "hash": tx.get("hash"),
                "sender": tx.get("sender"),
                "sysfee": sysfee,
                "netfee": netfee,
                "total_fee": total_fee,
                "attributes": tx.get("attributes") or [],
            }
        )
    return matches


def build_diagnosis(
    failure: dict,
    *,
    reference_block: dict | None = None,
    status: dict | None = None,
    local_gas_balances: list[dict] | None = None,
) -> dict:
    diagnosis = {
        "failure": failure,
        "status": status,
        "classification": "gas_burn_failure_unenriched",
        "recommendation": (
            "Capture the reference block and local sender balance, then restore "
            "a known-good checkpoint before the failed height or replay from a clean data directory."
        ),
    }
    if reference_block is None:
        if local_gas_balances:
            diagnosis["local"] = {"gas_balances": local_gas_balances}
        return diagnosis

    matches = matching_fee_transactions(reference_block, failure["burn_amount"])
    diagnosis["reference"] = {
        "block_hash": reference_block.get("hash"),
        "primary": reference_block.get("primary"),
        "tx_count": len(reference_block.get("tx") or []),
        "matching_fee_transactions": matches,
    }
    if local_gas_balances:
        diagnosis["local"] = {"gas_balances": local_gas_balances}
    if matches and failure["balance"] < failure["burn_amount"]:
        diagnosis["classification"] = "local_state_divergence"
        diagnosis["recommendation"] = (
            "The reference block contains a transaction whose system+network fee "
            "matches the failed burn amount, but the local GAS balance is lower. "
            "Treat this data directory as divergent: restore a validated checkpoint "
            "before the failed height or replay from clean state, then rerun state-root validation."
        )
    return diagnosis


def fetch_local_gas_balances(
    *,
    local_rpc: str,
    transactions: list[dict],
    address_version: int,
) -> list[dict]:
    balances = []
    seen_senders = set()
    for tx in transactions:
        sender = tx.get("sender")
        if not sender or sender in seen_senders:
            continue
        seen_senders.add(sender)
        item = {"sender": sender}
        try:
            storage_key = gas_account_storage_key(
                sender,
                address_version=address_version,
            )
            item["gas_account_key_base64"] = storage_key
            value = rpc_call(local_rpc, "getstorage", ["GasToken", storage_key])
            item["value_base64"] = value
            item["balance"] = decode_nep17_account_balance(value)
        except Exception as exc:  # pylint: disable=broad-except
            item["error"] = str(exc)
        balances.append(item)
    return balances


def load_status(path: Path | None) -> dict | None:
    if path is None or not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def run_diagnosis(
    *,
    log_path: Path,
    status_path: Path | None,
    reference_rpc: str | None,
    local_rpc: str | None = None,
    address_version: int = DEFAULT_ADDRESS_VERSION,
) -> dict:
    failures = parse_gas_burn_failures(log_path.read_text(encoding="utf-8", errors="replace"))
    if not failures:
        return {
            "classification": "no_gas_burn_failure_found",
            "failure": None,
            "status": load_status(status_path),
        }

    failure = failures[0]
    reference_block = None
    local_gas_balances = None
    if reference_rpc:
        reference_block = rpc_call(reference_rpc, "getblock", [failure["height"], 1])
    if local_rpc and reference_block:
        local_gas_balances = fetch_local_gas_balances(
            local_rpc=local_rpc,
            transactions=matching_fee_transactions(reference_block, failure["burn_amount"]),
            address_version=address_version,
        )
    return build_diagnosis(
        failure,
        reference_block=reference_block,
        status=load_status(status_path),
        local_gas_balances=local_gas_balances,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Diagnose native persistence failures such as GasToken::burn balance faults."
    )
    parser.add_argument("--log", required=True, help="neo-node log file to scan")
    parser.add_argument("--status-file", default=None, help="Optional state-root validator status JSON")
    parser.add_argument(
        "--reference-rpc",
        default=DEFAULT_REFERENCE_RPC,
        help=f"Reference Neo RPC used to fetch the failed block (default: {DEFAULT_REFERENCE_RPC})",
    )
    parser.add_argument(
        "--local-rpc",
        default=None,
        help="Optional local neo-rs RPC used to sample matching senders' GAS storage balances.",
    )
    parser.add_argument(
        "--address-version",
        default=DEFAULT_ADDRESS_VERSION,
        type=lambda value: int(value, 0),
        help=f"Neo address version for local GAS key construction (default: 0x{DEFAULT_ADDRESS_VERSION:02x})",
    )
    parser.add_argument(
        "--no-reference",
        action="store_true",
        help="Skip reference RPC enrichment and only parse the log/status files.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        payload = run_diagnosis(
            log_path=Path(args.log),
            status_path=Path(args.status_file) if args.status_file else None,
            reference_rpc=None if args.no_reference else args.reference_rpc,
            local_rpc=args.local_rpc,
            address_version=args.address_version,
        )
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
