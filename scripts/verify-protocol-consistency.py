#!/usr/bin/env python3
"""
Protocol consistency orchestrator for neo-rs ↔ C# Neo N3.

See docs/PROTOCOL_CONSISTENCY.md.

Subcommands:
  refresh-goldens  Fetch C# getstateroot for checkpoint heights into the
                   committed golden file (fail if any height missing).
  compare-rpc      Contiguous fail-closed compare of local Rust vs C# RPC.
  compare-goldens  Sparse compare of local Rust vs committed C# goldens
                   (fast while syncing; not a contiguous L3 claim).
  report-status    Print claimable verified range language from a compare report.
"""

from __future__ import annotations

import argparse
import gzip
import json
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
GOLDENS_PATH = (
    REPO_ROOT / "tests" / "fixtures" / "protocol_consistency" / "csharp_stateroot_goldens.jsonl"
)
REPORT_DIR = REPO_ROOT / "docs" / "protocol-consistency" / "reports"

# Dense early chain + historically sensitive heights from reproducers / plans.
DEFAULT_GOLDEN_HEIGHTS = sorted(
    set(
        list(range(0, 101))
        + list(range(1000, 1101))
        + [
            4410,
            14480,
            14492,
            14498,
            14500,
            14510,
            21288,
            21373,
            38781,
            38791,
            38883,
            52950,
            55100,
            172612,
            172613,
            203262,
            274157,
            294369,
            676050,
            679779,
            980195,
            980196,
            1074782,
            1208916,
            1268131,
            1283521,
            1394579,
            1465790,
        ]
    )
)


def rpc_call(url: str, method: str, params: list, timeout: float = 30.0, retries: int = 6):
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
            "User-Agent": "neo-rs-protocol-consistency/1.0",
        },
        method="POST",
    )
    last_err: Exception | None = None
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
            if raw.startswith(b"\x1f\x8b"):
                raw = gzip.decompress(raw)
            body = json.loads(raw.decode("utf-8"))
            if "error" in body:
                raise RuntimeError(f"{method} error from {url}: {body['error']}")
            return body["result"]
        except Exception as exc:  # noqa: BLE001 - fail-closed after retries
            last_err = exc
            time.sleep(0.4 * (attempt + 1))
    raise RuntimeError(f"{method} failed against {url}: {last_err}")


def normalize_root(value: str | None) -> str | None:
    if value is None:
        return None
    text = value.strip().lower()
    if not text.startswith("0x"):
        text = "0x" + text
    return text


def fetch_stateroot(url: str, height: int) -> dict:
    result = rpc_call(url, "getstateroot", [height])
    if not isinstance(result, dict):
        raise RuntimeError(f"getstateroot({height}) returned non-object: {result!r}")
    # C# seeds typically use "roothash"; neo-rs JSON uses "rootHash".
    root = normalize_root(result.get("roothash") or result.get("rootHash"))
    if not root:
        raise RuntimeError(f"getstateroot({height}) missing roothash/rootHash")
    return {
        "height": int(result.get("index", height)),
        "version": int(result.get("version", 0)),
        "roothash": root,
        "source": url,
    }


def cmd_refresh_goldens(args: argparse.Namespace) -> int:
    url = args.reference
    heights = DEFAULT_GOLDEN_HEIGHTS
    if args.extra_end is not None and args.extra_end >= 0:
        heights = sorted(set(heights) | set(range(0, args.extra_end + 1)))

    GOLDENS_PATH.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    failures = []
    for height in heights:
        try:
            row = fetch_stateroot(url, height)
            rows.append(row)
            print(f"OK  {height}\t{row['roothash']}")
        except Exception as exc:  # noqa: BLE001
            failures.append((height, str(exc)))
            print(f"ERR {height}\t{exc}", file=sys.stderr)

    if failures:
        print(f"FAIL: {len(failures)} heights could not be fetched", file=sys.stderr)
        return 1

    meta = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "reference": url,
        "count": len(rows),
        "purpose": "C# oracle state-root goldens for protocol consistency gates",
    }
    with GOLDENS_PATH.open("w", encoding="utf-8") as fh:
        fh.write(json.dumps({"_meta": meta}) + "\n")
        for row in rows:
            fh.write(json.dumps(row, sort_keys=True) + "\n")
    print(f"Wrote {len(rows)} goldens -> {GOLDENS_PATH}")
    return 0


def load_goldens(path: Path) -> dict[int, str]:
    roots: dict[int, str] = {}
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            obj = json.loads(line)
            if "_meta" in obj:
                continue
            roots[int(obj["height"])] = normalize_root(obj["roothash"])  # type: ignore[arg-type]
    return roots


def cmd_compare_rpc(args: argparse.Namespace) -> int:
    local = args.local
    reference = args.reference
    start = args.start
    end = args.end
    if end < start:
        print("end must be >= start", file=sys.stderr)
        return 2

    mismatches = []
    missing_local = []
    missing_ref = []
    matched = 0

    for height in range(start, end + 1):
        try:
            csharp = fetch_stateroot(reference, height)
        except Exception as exc:  # noqa: BLE001
            missing_ref.append({"height": height, "error": str(exc)})
            print(f"REF-MISS {height}\t{exc}", file=sys.stderr)
            continue
        try:
            rust = fetch_stateroot(local, height)
        except Exception as exc:  # noqa: BLE001
            missing_local.append({"height": height, "error": str(exc)})
            print(f"LOCAL-MISS {height}\t{exc}", file=sys.stderr)
            continue

        if rust["roothash"] != csharp["roothash"]:
            mismatches.append(
                {
                    "height": height,
                    "rust": rust["roothash"],
                    "csharp": csharp["roothash"],
                }
            )
            print(
                f"MISMATCH {height}\trust={rust['roothash']}\tcsharp={csharp['roothash']}",
                file=sys.stderr,
            )
        else:
            matched += 1
            if args.verbose or height % 100 == 0 or height == end:
                print(f"MATCH {height}\t{rust['roothash']}")

    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "local": local,
        "reference": reference,
        "start": start,
        "end": end,
        "matched": matched,
        "mismatches": mismatches,
        "missing_local": missing_local,
        "missing_reference": missing_ref,
        "claimable_contiguous": None,
    }

    # Contiguous claimable prefix from start with zero failures.
    claim_end = start - 1
    bad = {m["height"] for m in mismatches} | {
        m["height"] for m in missing_local
    } | {m["height"] for m in missing_ref}
    for height in range(start, end + 1):
        if height in bad:
            break
        claim_end = height
    if claim_end >= start:
        report["claimable_contiguous"] = {"start": start, "end": claim_end}

    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    report_path = REPORT_DIR / f"compare-rpc-{start}-{end}-{stamp}.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Report -> {report_path}")

    if mismatches or missing_local or missing_ref:
        print(
            "FAIL: protocol consistency not proven for full requested range "
            f"(matched={matched}, mismatches={len(mismatches)}, "
            f"missing_local={len(missing_local)}, missing_ref={len(missing_ref)})",
            file=sys.stderr,
        )
        if report["claimable_contiguous"]:
            c = report["claimable_contiguous"]
            print(
                f"Partial claimable contiguous only: [{c['start']}, {c['end']}]",
                file=sys.stderr,
            )
        return 1

    print(f"PASS: contiguous [{start}, {end}] matched C# state roots ({matched} heights)")
    return 0


def cmd_compare_goldens(args: argparse.Namespace) -> int:
    """Compare local neo-rs roots to committed C# goldens (heights <= local tip)."""
    if not GOLDENS_PATH.exists():
        print(f"missing goldens: {GOLDENS_PATH}", file=sys.stderr)
        return 2

    tip = int(rpc_call(args.local, "getblockcount", [])) - 1
    goldens = load_goldens(GOLDENS_PATH)
    within = sorted(h for h in goldens if h <= tip)
    mismatches = []
    missing_local = []
    matched = 0

    for height in within:
        try:
            rust = fetch_stateroot(args.local, height)
        except Exception as exc:  # noqa: BLE001
            missing_local.append({"height": height, "error": str(exc)})
            print(f"LOCAL-MISS {height}\t{exc}", file=sys.stderr)
            continue
        expected = goldens[height]
        if rust["roothash"] != expected:
            mismatches.append(
                {
                    "height": height,
                    "rust": rust["roothash"],
                    "csharp_golden": expected,
                }
            )
            print(
                f"MISMATCH {height}\trust={rust['roothash']}\tgolden={expected}",
                file=sys.stderr,
            )
        else:
            matched += 1
            if args.verbose:
                print(f"MATCH {height}\t{rust['roothash']}")

    bad_heights = {m["height"] for m in mismatches} | {m["height"] for m in missing_local}
    highest_matched = max((h for h in within if h not in bad_heights), default=None)
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "local": args.local,
        "local_tip": tip,
        "goldens_le_tip": len(within),
        "matched": matched,
        "mismatches": mismatches,
        "missing_local": missing_local,
        "highest_matched_golden": highest_matched,
    }
    report_path = REPORT_DIR / f"compare-goldens-{tip}-{stamp}.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Report -> {report_path}")

    if mismatches or missing_local:
        print(
            "FAIL: golden compare incomplete "
            f"(matched={matched}, mismatches={len(mismatches)}, "
            f"missing_local={len(missing_local)})",
            file=sys.stderr,
        )
        return 1

    print(
        f"PASS: {matched} goldens <= tip {tip} match committed C# checkpoints "
        f"(sparse L2 evidence; not contiguous L3)"
    )
    return 0


def cmd_verify_goldens_online(args: argparse.Namespace) -> int:
    """Re-fetch each committed golden against live C# and fail on drift."""
    if not GOLDENS_PATH.exists():
        print(f"missing goldens: {GOLDENS_PATH}", file=sys.stderr)
        return 2
    goldens = load_goldens(GOLDENS_PATH)
    mismatches = 0
    for height, expected in sorted(goldens.items()):
        got = fetch_stateroot(args.reference, height)["roothash"]
        if got != expected:
            print(f"DRIFT {height}\tgolden={expected}\tlive={got}", file=sys.stderr)
            mismatches += 1
        elif args.verbose:
            print(f"OK {height}")
    if mismatches:
        print(f"FAIL: {mismatches} golden heights drifted from live C#", file=sys.stderr)
        return 1
    print(f"PASS: {len(goldens)} goldens match live C# at {args.reference}")
    return 0


def cmd_report_status(args: argparse.Namespace) -> int:
    path = Path(args.report)
    report = json.loads(path.read_text(encoding="utf-8"))
    claim = report.get("claimable_contiguous")
    print(f"report: {path}")
    print(f"matched: {report.get('matched')}")
    print(f"mismatches: {len(report.get('mismatches') or [])}")
    if claim:
        print(
            "claimable_language: "
            f"neo-rs state roots match C# for contiguous MainNet heights "
            f"[{claim['start']}, {claim['end']}] (fail-closed compare)."
        )
    else:
        print("claimable_language: none (no contiguous zero-failure prefix)")
    full = (
        not report.get("mismatches")
        and not report.get("missing_local")
        and not report.get("missing_reference")
    )
    if full:
        print(
            f"full_range_pass: [{report.get('start')}, {report.get('end')}]"
        )
    return 0 if full else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_refresh = sub.add_parser("refresh-goldens", help="Fetch C# golden checkpoints")
    p_refresh.add_argument(
        "--reference",
        default="http://seed1.neo.org:10332",
        help="C# / official seed RPC",
    )
    p_refresh.add_argument(
        "--extra-end",
        type=int,
        default=None,
        help="Also include every height in [0, extra-end]",
    )
    p_refresh.set_defaults(func=cmd_refresh_goldens)

    p_cmp = sub.add_parser("compare-rpc", help="Fail-closed Rust vs C# RPC compare")
    p_cmp.add_argument("--local", required=True, help="neo-rs RPC URL")
    p_cmp.add_argument(
        "--reference",
        default="http://seed1.neo.org:10332",
        help="C# reference RPC URL",
    )
    p_cmp.add_argument("--start", type=int, default=0)
    p_cmp.add_argument("--end", type=int, required=True)
    p_cmp.add_argument("--verbose", action="store_true")
    p_cmp.set_defaults(func=cmd_compare_rpc)

    p_cg = sub.add_parser(
        "compare-goldens",
        help="Compare local neo-rs roots to committed C# golden checkpoints",
    )
    p_cg.add_argument("--local", required=True, help="neo-rs RPC URL")
    p_cg.add_argument("--verbose", action="store_true")
    p_cg.set_defaults(func=cmd_compare_goldens)

    p_g = sub.add_parser(
        "verify-goldens-online",
        help="Re-check committed goldens against live C# (CI-friendly)",
    )
    p_g.add_argument("--reference", default="http://seed1.neo.org:10332")
    p_g.add_argument("--verbose", action="store_true")
    p_g.set_defaults(func=cmd_verify_goldens_online)

    p_r = sub.add_parser("report-status", help="Summarize a compare report")
    p_r.add_argument("--report", required=True)
    p_r.set_defaults(func=cmd_report_status)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    sys.exit(main())
