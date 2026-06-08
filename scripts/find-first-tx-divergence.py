#!/usr/bin/env python3
"""Compare transaction execution notifications between Rust trace logs and C# RPC.

The Rust node emits structured trace lines when NEO_TRACE_BLOCK is set:
  - "TRACE: tx execution result" with fields: block_index, tx_hash, vm_state, gas_consumed, notif_count, exception
  - "TRACE: notification" with fields: block_index, tx_hash, notif_idx, contract, event, state

The C# reference provides the same data via getapplicationlog RPC.

Usage:
    python3 scripts/find-first-tx-divergence.py --block 125000 --rust-log /tmp/import_trace.log
    python3 scripts/find-first-tx-divergence.py --block 125000-125100 --rust-log /tmp/import_trace.log
"""

import argparse
import gzip
import json
import re
import sys
import urllib.request
import time

CSHARP_RPC = "http://seed1.neo.org:10332"

# Strip ANSI escape codes that tracing-subscriber may emit.
ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")

# ── Rust log parsing ──────────────────────────────────────────────────────────

# The tracing crate with the default subscriber emits lines like:
#   2026-04-11T10:00:00.000Z  WARN neo: TRACE: tx execution result block_index=125000 tx_hash=0xabc... vm_state=HALT gas_consumed=1234 notif_count=2 exception=None
#   2026-04-11T10:00:00.000Z  WARN neo: TRACE: notification block_index=125000 tx_hash=0xabc... notif_idx=0 contract=0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5 event=Transfer state=["bytes:0x...", "int:100"]
#
# The exact format depends on tracing-subscriber config; we parse key=value pairs.

def strip_ansi(line: str) -> str:
    return ANSI_RE.sub("", line)


def parse_kv_pairs(line: str) -> dict:
    """Extract key=value pairs from a structured tracing log line.

    Handles both unquoted scalars and quoted/bracketed compound values.
    """
    result = {}
    cleaned = strip_ansi(line).rstrip()
    i = 0
    n = len(cleaned)
    while i < n:
        # Skip whitespace and commas
        while i < n and cleaned[i] in (" ", "\t", ","):
            i += 1
        if i >= n:
            break
        # Look for key=
        m = re.match(r"([a-zA-Z_][a-zA-Z0-9_]*)=", cleaned[i:])
        if not m:
            i += 1
            continue
        key = m.group(1)
        i += m.end()
        if i >= n:
            break
        # Parse value
        if cleaned[i] == '"':
            # Quoted string
            end = cleaned.index('"', i + 1) if '"' in cleaned[i + 1:] else n
            result[key] = cleaned[i + 1:end]
            i = end + 1
        elif cleaned[i] == '[':
            # Bracketed array
            depth = 0
            start = i
            while i < n:
                if cleaned[i] == '[':
                    depth += 1
                elif cleaned[i] == ']':
                    depth -= 1
                    if depth == 0:
                        i += 1
                        break
                i += 1
            result[key] = cleaned[start:i]
        else:
            # Unquoted value – ends at next whitespace or comma
            end = i
            while end < n and cleaned[end] not in (" ", "\t", ","):
                end += 1
            result[key] = cleaned[i:end]
            i = end
    return result


def parse_rust_log(log_path: str, blocks: set[int] | None = None):
    """Parse Rust trace log into per-block per-tx execution data.

    Returns:
        dict[int, dict[str, RustTxResult]]  block_index -> tx_hash -> result
    """
    results = {}  # block -> {tx_hash -> {"vm_state", "gas_consumed", "notif_count", "exception", "notifications": [...]}}
    with open(log_path, "r", errors="replace") as f:
        for raw_line in f:
            line = strip_ansi(raw_line)
            if "TRACE: tx execution result" in line:
                kv = parse_kv_pairs(line)
                block_idx = int(kv.get("block_index", -1))
                if blocks is not None and block_idx not in blocks:
                    continue
                tx_hash = kv.get("tx_hash", "")
                if block_idx not in results:
                    results[block_idx] = {}
                results[block_idx][tx_hash] = {
                    "vm_state": kv.get("vm_state", "").strip('"'),
                    "gas_consumed": kv.get("gas_consumed", ""),
                    "notif_count": int(kv.get("notif_count", 0)),
                    "exception": kv.get("exception", "None"),
                    "notifications": [],
                }
            elif "TRACE: notification" in line:
                kv = parse_kv_pairs(line)
                block_idx = int(kv.get("block_index", -1))
                if blocks is not None and block_idx not in blocks:
                    continue
                tx_hash = kv.get("tx_hash", "")
                if block_idx not in results or tx_hash not in results[block_idx]:
                    continue
                notif = {
                    "notif_idx": int(kv.get("notif_idx", -1)),
                    "contract": kv.get("contract", ""),
                    "event": kv.get("event", "").strip('"'),
                    "state_raw": kv.get("state", "[]"),
                }
                results[block_idx][tx_hash]["notifications"].append(notif)
    return results


# ── C# RPC helpers ────────────────────────────────────────────────────────────

def rpc_call(url: str, method: str, params: list, timeout: int = 30, retries: int = 5):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=payload,
        headers={
            "Content-Type": "application/json",
            "Accept-Encoding": "identity",
            "User-Agent": "neo-rs-divergence-finder/1.0",
        },
        method="POST",
    )
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
                if raw.startswith(b"\x1f\x8b"):
                    raw = gzip.decompress(raw)
                data = json.loads(raw.decode("utf-8"))
            if "error" in data and data["error"]:
                if attempt < retries - 1:
                    time.sleep(1)
                    continue
                return None, data["error"]
            return data.get("result"), None
        except Exception as e:
            if attempt < retries - 1:
                time.sleep(2)
                continue
            return None, str(e)
    return None, "max retries reached"


def get_block_tx_hashes(url: str, height: int) -> tuple[list[str] | None, str | None]:
    block, err = rpc_call(url, "getblock", [height, True])
    if err or not block:
        return None, err or "empty block response"
    tx_hashes = [tx["hash"] for tx in block.get("tx", [])]
    return tx_hashes, None


def get_csharp_applog(url: str, tx_hash: str):
    result, err = rpc_call(url, "getapplicationlog", [tx_hash])
    if err:
        return None, err
    if not result:
        return None, "empty result"
    return result, None


def extract_csharp_notifications(applog: dict) -> list[dict]:
    """Extract notifications from C# getapplicationlog response.

    Returns a list of dicts with: contract, event, state_count
    """
    executions = applog.get("executions") or []
    if not executions:
        return []
    exec0 = executions[0]
    notifs = exec0.get("notifications") or []
    result = []
    for n in notifs:
        state = n.get("state", {})
        state_values = state.get("value") or []
        result.append({
            "contract": n.get("contract", ""),
            "event": n.get("eventname", ""),
            "state_count": len(state_values),
            "state_types": [v.get("type", "") for v in state_values],
        })
    return result


def extract_csharp_vm_state(applog: dict) -> str:
    executions = applog.get("executions") or []
    if not executions:
        return "UNKNOWN"
    return executions[0].get("vmstate", "UNKNOWN")


def extract_csharp_gas(applog: dict) -> str:
    executions = applog.get("executions") or []
    if not executions:
        return ""
    return executions[0].get("gasconsumed", "")


# ── Rust notification parsing ─────────────────────────────────────────────────

def count_rust_state_items(state_raw: str) -> int:
    """Count items in the Rust state array string like '["bytes:0x...", "int:100"]'."""
    s = state_raw.strip()
    if s in ("[]", ""):
        return 0
    # Count comma-separated items within brackets
    inner = s.strip("[]")
    if not inner.strip():
        return 0
    # Split on ", " but respect quoted strings
    items = []
    depth = 0
    current = []
    for ch in inner:
        if ch == '"':
            depth = 1 - depth
        elif ch == ',' and depth == 0:
            items.append("".join(current).strip())
            current = []
            continue
        current.append(ch)
    last = "".join(current).strip()
    if last:
        items.append(last)
    return len(items)


# ── Comparison ────────────────────────────────────────────────────────────────

def compare_tx(tx_hash: str, rust_data: dict, csharp_applog: dict) -> list[str]:
    """Compare a single transaction. Returns list of difference descriptions."""
    diffs = []

    # Compare VM state
    rust_vm = rust_data["vm_state"]
    csharp_vm = extract_csharp_vm_state(csharp_applog)
    if rust_vm != csharp_vm:
        diffs.append(f"  vm_state: Rust={rust_vm} C#={csharp_vm}")

    # Compare gas consumed.
    # Both Rust and C# report gas as an integer string in the smallest unit.
    # Some C# versions may use a decimal string (e.g. "10.1377868"); handle both.
    rust_gas = rust_data["gas_consumed"]
    csharp_gas = extract_csharp_gas(csharp_applog)
    try:
        rust_gas_int = int(rust_gas)
        # C# may return either an integer string or a decimal string
        if "." in csharp_gas:
            csharp_gas_int = int(round(float(csharp_gas) * 100_000_000))
        else:
            csharp_gas_int = int(csharp_gas) if csharp_gas else -1
        if rust_gas_int != csharp_gas_int:
            diffs.append(f"  gas_consumed: Rust={rust_gas_int} C#={csharp_gas_int} (raw: Rust={rust_gas} C#={csharp_gas})")
    except (ValueError, TypeError):
        # If formats don't match, just report raw
        if rust_gas != csharp_gas:
            diffs.append(f"  gas_consumed: Rust={rust_gas} C#={csharp_gas} (could not normalize)")

    # Compare notification count
    csharp_notifs = extract_csharp_notifications(csharp_applog)
    rust_notif_count = rust_data["notif_count"]
    csharp_notif_count = len(csharp_notifs)
    if rust_notif_count != csharp_notif_count:
        diffs.append(f"  notification_count: Rust={rust_notif_count} C#={csharp_notif_count}")

    # Compare individual notifications
    rust_notifs = rust_data["notifications"]
    min_count = min(len(rust_notifs), len(csharp_notifs))
    for i in range(min_count):
        rn = rust_notifs[i]
        cn = csharp_notifs[i]

        # Compare contract hash
        rust_contract = rn["contract"].lower()
        csharp_contract = cn["contract"].lower()
        if rust_contract != csharp_contract:
            diffs.append(f"  notification[{i}] contract: Rust={rust_contract} C#={csharp_contract}")

        # Compare event name
        rust_event = rn["event"]
        csharp_event = cn["event"]
        if rust_event != csharp_event:
            diffs.append(f"  notification[{i}] event: Rust={rust_event} C#={csharp_event}")

        # Compare state item count
        rust_state_count = count_rust_state_items(rn["state_raw"])
        csharp_state_count = cn["state_count"]
        if rust_state_count != csharp_state_count:
            diffs.append(f"  notification[{i}] state_items: Rust={rust_state_count} C#={csharp_state_count}")

    # Report extra notifications on either side
    if len(rust_notifs) > len(csharp_notifs):
        for i in range(len(csharp_notifs), len(rust_notifs)):
            rn = rust_notifs[i]
            diffs.append(f"  notification[{i}] EXTRA in Rust: contract={rn['contract']} event={rn['event']}")
    elif len(csharp_notifs) > len(rust_notifs):
        for i in range(len(rust_notifs), len(csharp_notifs)):
            cn = csharp_notifs[i]
            diffs.append(f"  notification[{i}] EXTRA in C#: contract={cn['contract']} event={cn['event']}")

    return diffs


def process_block(block_height: int, rust_block_data: dict, csharp_url: str) -> tuple[int, int, list[str]]:
    """Process a single block. Returns (total_txs, divergent_txs, report_lines)."""
    tx_hashes, err = get_block_tx_hashes(csharp_url, block_height)
    if err:
        return 0, 0, [f"  ERROR: could not fetch block {block_height} from C# RPC: {err}"]

    if not tx_hashes:
        # No transactions in block
        rust_tx_count = len(rust_block_data) if rust_block_data else 0
        if rust_tx_count > 0:
            return 0, 1, [f"  ERROR: C# has 0 transactions but Rust logged {rust_tx_count}"]
        return 0, 0, []

    total_txs = len(tx_hashes)
    divergent_txs = 0
    lines = []

    for tx_hash in tx_hashes:
        # Find Rust data for this tx
        # Rust tx_hash may or may not have 0x prefix; normalize
        rust_tx = None
        for rh, rd in (rust_block_data or {}).items():
            rh_norm = rh.lower() if rh.startswith("0x") else "0x" + rh.lower()
            th_norm = tx_hash.lower()
            if rh_norm == th_norm:
                rust_tx = rd
                break

        if rust_tx is None:
            lines.append(f"  TX {tx_hash}: NOT FOUND in Rust log (block may not have been traced)")
            divergent_txs += 1
            continue

        # Fetch C# application log
        applog, err = get_csharp_applog(csharp_url, tx_hash)
        if err:
            lines.append(f"  TX {tx_hash}: C# getapplicationlog failed: {err}")
            divergent_txs += 1
            continue

        diffs = compare_tx(tx_hash, rust_tx, applog)
        if diffs:
            divergent_txs += 1
            lines.append(f"  TX {tx_hash}: DIVERGENT")
            lines.extend(diffs)
        else:
            lines.append(f"  TX {tx_hash}: OK")

    # Check for Rust txs not in C# block (shouldn't happen but report it)
    csharp_set = {h.lower() for h in tx_hashes}
    for rh in (rust_block_data or {}):
        rh_norm = rh.lower() if rh.startswith("0x") else "0x" + rh.lower()
        if rh_norm not in csharp_set:
            lines.append(f"  TX {rh}: IN RUST LOG but NOT in C# block")
            divergent_txs += 1

    return total_txs, divergent_txs, lines


def main():
    parser = argparse.ArgumentParser(
        description="Find transaction execution divergences between Rust trace logs and C# reference."
    )
    parser.add_argument(
        "--block",
        required=True,
        help="Block height or range (e.g. 125000 or 125000-125100)",
    )
    parser.add_argument(
        "--rust-log",
        required=True,
        help="Path to Rust import trace log file",
    )
    parser.add_argument(
        "--csharp-rpc",
        default=CSHARP_RPC,
        help=f"C# reference RPC endpoint (default: {CSHARP_RPC})",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Show OK transactions too (default: only divergent)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        dest="json_output",
        help="Output results as JSON",
    )
    args = parser.parse_args()

    # Parse block range
    block_str = args.block.strip()
    if "-" in block_str:
        parts = block_str.split("-", 1)
        block_start = int(parts[0])
        block_end = int(parts[1])
    else:
        block_start = int(block_str)
        block_end = block_start

    blocks = set(range(block_start, block_end + 1))

    print(f"Parsing Rust log: {args.rust_log}", file=sys.stderr)
    rust_data = parse_rust_log(args.rust_log, blocks)
    print(f"Found trace data for {len(rust_data)} block(s) in log", file=sys.stderr)

    total_blocks = 0
    total_txs = 0
    total_divergent = 0
    first_divergent_block = None
    json_results = []

    for height in range(block_start, block_end + 1):
        total_blocks += 1
        rust_block = rust_data.get(height, {})

        txs, divs, lines = process_block(height, rust_block, args.csharp_rpc)
        total_txs += txs
        total_divergent += divs

        if divs > 0 and first_divergent_block is None:
            first_divergent_block = height

        has_content = divs > 0 or (args.verbose and txs > 0)
        if not rust_block and txs > 0:
            has_content = True

        if args.json_output:
            json_results.append({
                "block": height,
                "total_txs": txs,
                "divergent_txs": divs,
                "details": lines,
            })
        else:
            if has_content:
                status = "DIVERGENT" if divs > 0 else "OK"
                print(f"\n=== Block {height} ({txs} txs) [{status}] ===")
                for line in lines:
                    if args.verbose or "DIVERGENT" in line or "EXTRA" in line or "ERROR" in line or "NOT FOUND" in line:
                        print(line)

    # Summary
    if args.json_output:
        summary = {
            "block_range": f"{block_start}-{block_end}",
            "total_blocks": total_blocks,
            "total_txs": total_txs,
            "divergent_txs": total_divergent,
            "first_divergent_block": first_divergent_block,
            "blocks": json_results,
        }
        print(json.dumps(summary, indent=2))
    else:
        print(f"\n{'='*60}")
        print(f"Summary: {block_start}-{block_end}")
        print(f"  Blocks scanned:       {total_blocks}")
        print(f"  Total transactions:   {total_txs}")
        print(f"  Divergent txs:        {total_divergent}")
        if first_divergent_block is not None:
            print(f"  First divergence at:  block {first_divergent_block}")
        else:
            print(f"  First divergence at:  (none found)")
        print(f"{'='*60}")

    if total_divergent > 0:
        sys.exit(1)
    sys.exit(0)


if __name__ == "__main__":
    main()
