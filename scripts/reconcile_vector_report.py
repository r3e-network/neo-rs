#!/usr/bin/env python3

from __future__ import annotations

import gzip
import json
import sys
import urllib.request
from pathlib import Path


POLICY_VECTORS = {
    "Policy_getFeePerByte": "getFeePerByte",
    "Policy_getExecFeeFactor": "getExecFeeFactor",
    "Policy_getStoragePrice": "getStoragePrice",
}


def rpc_call(rpc: str, method: str, params: list):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(
        "utf-8"
    )
    req = urllib.request.Request(
        rpc,
        data=payload,
        headers={"Content-Type": "application/json", "Accept-Encoding": "identity"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=15) as resp:
        raw = resp.read()
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    parsed = json.loads(raw.decode("utf-8"))
    if "error" in parsed:
        raise RuntimeError(f"rpc error {method}: {parsed['error']}")
    return parsed.get("result")


def policy_values(rpc: str):
    contracts = rpc_call(rpc, "getnativecontracts", [])
    if not isinstance(contracts, list):
        raise RuntimeError("unexpected getnativecontracts result")

    policy_hash = None
    for contract in contracts:
        manifest = contract.get("manifest") if isinstance(contract, dict) else None
        if isinstance(manifest, dict) and manifest.get("name") == "PolicyContract":
            policy_hash = contract.get("hash")
            break

    if not policy_hash:
        raise RuntimeError("policy contract hash not found")

    values = {}
    for vector, method in POLICY_VECTORS.items():
        result = rpc_call(rpc, "invokefunction", [policy_hash, method, []])
        if not isinstance(result, dict) or result.get("state") != "HALT":
            raise RuntimeError(f"{method} did not HALT")
        stack = result.get("stack")
        if not isinstance(stack, list) or not stack:
            raise RuntimeError(f"{method} returned empty stack")
        value = stack[0].get("value") if isinstance(stack[0], dict) else None
        if value is None:
            raise RuntimeError(f"{method} returned no value")
        values[vector] = str(value)

    return {
        "rpc": rpc,
        "policy_hash": policy_hash,
        "values": values,
    }


def _update_summary(report: dict) -> None:
    results = report.get("results") or []
    summary = report.get("summary") if isinstance(report.get("summary"), dict) else {}
    total = len(results)
    passed = sum(1 for entry in results if entry.get("match") is True)
    failed = sum(1 for entry in results if entry.get("match") is False)
    errors = sum(1 for entry in results if entry.get("match") is None)
    summary["total"] = total
    summary["passed"] = passed
    summary["failed"] = failed
    summary["errors"] = errors
    summary["pass_rate"] = f"{(passed * 100.0 / total):.2f}%" if total else "0.00%"
    report["summary"] = summary


def _policy_vector_matches(failure: dict, live: dict, local: dict) -> bool:
    vector = failure.get("vector")
    if vector not in POLICY_VECTORS:
        return False
    diffs = failure.get("differences") or []
    if len(diffs) != 1:
        return False
    diff = diffs[0]
    return (
        diff.get("type") == "stack_value"
        and diff.get("path") == "stack[0]"
        and str(diff.get("python")) == live["values"][vector]
        and str(diff.get("csharp")) == local["values"][vector]
    )


def _gas_mismatch_matches_policy_ratio(failure: dict, live: dict, local: dict) -> bool:
    diffs = failure.get("differences") or []
    if not diffs:
        return False

    try:
        live_exec = int(live["values"]["Policy_getExecFeeFactor"])
        local_exec = int(local["values"]["Policy_getExecFeeFactor"])
    except (KeyError, TypeError, ValueError):
        return False

    if live_exec <= 0 or local_exec <= 0 or live_exec == local_exec:
        return False

    for diff in diffs:
        if diff.get("type") != "gas_mismatch" or diff.get("path") != "gas_consumed":
            return False
        try:
            python_gas = int(diff.get("python"))
            local_gas = int(diff.get("csharp"))
        except (TypeError, ValueError):
            return False
        if local_gas * live_exec != python_gas * local_exec:
            return False

    return True


def reconcile_report(report: dict, live: dict, local: dict, network_dir: Path) -> bool:
    results = report.get("results") or []
    failures = [entry for entry in results if entry.get("match") is False]
    if not failures:
        return False

    reconciled_vectors: list[str] = []
    for failure in failures:
        vector = str(failure.get("vector"))
        if _policy_vector_matches(failure, live, local) or _gas_mismatch_matches_policy_ratio(
            failure, live, local
        ):
            reconciled_vectors.append(vector)
            continue
        return False

    raw_report_path = network_dir / "neo-rs-vectors.raw.json"
    if not raw_report_path.exists():
        raw_report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    for entry in results:
        if entry.get("vector") in reconciled_vectors:
            entry["match"] = True
            entry["differences"] = []

    state_file = network_dir / "policy-state-reconciliation.json"
    state_file.write_text(
        json.dumps(
            {
                "reason": "policy_values_differ_between_live_chain_and_unsynced_local_node",
                "vectors": sorted(reconciled_vectors),
                "live": live,
                "local": local,
                "raw_report": str(raw_report_path),
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    report["state_aware_adjustments"] = {
        "type": "policy_state_reconciliation",
        "details": str(state_file),
    }
    _update_summary(report)
    return True


def main(argv: list[str] | None = None) -> int:
    argv = argv or sys.argv[1:]
    if len(argv) != 4:
        print(
            "usage: reconcile_vector_report.py <report_path> <local_rpc> <csharp_rpc> <network_dir>",
            file=sys.stderr,
        )
        return 2

    report_path = Path(argv[0])
    local_rpc = argv[1]
    csharp_rpc = argv[2]
    network_dir = Path(argv[3])

    report = json.loads(report_path.read_text(encoding="utf-8"))
    try:
        live = policy_values(csharp_rpc)
        local = policy_values(local_rpc)
    except Exception:
        return 1

    if not reconcile_report(report, live, local, network_dir):
        return 1

    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
