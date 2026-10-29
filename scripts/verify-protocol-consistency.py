#!/usr/bin/env python3
"""
Protocol consistency orchestrator for neo-rs ↔ C# Neo N3.

See docs/PROTOCOL_CONSISTENCY.md.

Subcommands:
  refresh-goldens  Fetch C# getstateroot for checkpoint heights into the
                   committed golden file (fail if any height missing).
  compare-rpc      Contiguous fail-closed compare of local Rust vs C# RPC.
  compare-applogs  Fail-closed compare of block and tx getapplicationlog
                   (VM state, gas, exception, stack, notifications).
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


APPLOG_EXECUTION_FIELDS = ("trigger", "vmstate", "exception", "gasconsumed", "stack", "notifications")


def _normalize_json(value):
    if isinstance(value, dict):
        return {str(k).lower(): _normalize_json(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_normalize_json(v) for v in value]
    if isinstance(value, str) and value.lower().startswith("0x"):
        return value.lower()
    return value


def normalize_applog(log: dict) -> list[dict]:
    """Return the consensus-relevant execution records of a getapplicationlog result."""
    executions = _normalize_json(log).get("executions")
    if not isinstance(executions, list):
        raise ValueError("applicationlog has no executions list")
    return [
        {field: execution.get(field) for field in APPLOG_EXECUTION_FIELDS}
        for execution in executions
    ]


def applog_height_should_retry(
    mismatches: list, missing: list, attempt: int, limit: int
) -> bool:
    """Retry a height only when every failure was a fetch error.

    A field mismatch is deterministic evidence and must not be retried away.
    """
    return not mismatches and bool(missing) and attempt + 1 < limit


def applog_journal_covers(heights: list[int], rows: list[dict]) -> bool:
    """True only when every requested height has a latest successful comparison."""
    latest: dict[int, dict] = {}
    for row in rows:
        if "height" not in row:
            continue
        latest[int(row["height"])] = row
    if not heights:
        return False
    for height in heights:
        row = latest.get(height)
        if not row or not row.get("ok") or int(row.get("compared") or 0) < 1:
            return False
    return True


def diff_applog(rust: dict, csharp: dict) -> list[str]:
    """List every field where the Rust application log differs from C#."""
    rust_execs = normalize_applog(rust)
    csharp_execs = normalize_applog(csharp)
    diffs = []
    if len(rust_execs) != len(csharp_execs):
        diffs.append(f"executions: rust has {len(rust_execs)}, csharp has {len(csharp_execs)}")
    for index, (r, c) in enumerate(zip(rust_execs, csharp_execs)):
        for field in APPLOG_EXECUTION_FIELDS:
            if r[field] != c[field]:
                diffs.append(
                    f"executions[{index}].{field}: rust={json.dumps(r[field])[:300]} "
                    f"csharp={json.dumps(c[field])[:300]}"
                )
    return diffs


def _applog_journal_path(args: argparse.Namespace) -> Path:
    if args.journal:
        return Path(args.journal)
    return REPORT_DIR / f"compare-applogs-{args.start}-{args.end}-s{args.stride}.jsonl"


def _load_applog_journal(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    return rows


def _append_applog_journal(path: Path, row: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, ensure_ascii=False) + "\n")
        handle.flush()


def _write_applog_report(args: argparse.Namespace, heights: list[int], rows: list[dict]) -> tuple[Path, bool]:
    latest: dict[int, dict] = {}
    for row in rows:
        if "height" in row:
            latest[int(row["height"])] = row
    mismatches = []
    missing = []
    compared = 0
    for height in heights:
        row = latest.get(height)
        if not row:
            continue
        compared += int(row.get("compared") or 0)
        mismatches.extend(row.get("mismatches") or [])
        missing.extend(row.get("missing") or [])
    passed = applog_journal_covers(heights, rows)
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "kind": "applicationlog",
        "local": args.local,
        "reference": args.reference,
        "start": args.start,
        "end": args.end,
        "stride": args.stride,
        "heights": len(heights),
        "compared": compared,
        "mismatches": mismatches,
        "missing": missing,
        "journal": str(_applog_journal_path(args)),
        "pass": passed,
    }
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    report_path = REPORT_DIR / f"compare-applogs-{args.start}-{args.end}-s{args.stride}-{stamp}.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return report_path, passed


def cmd_compare_applogs(args: argparse.Namespace) -> int:
    """Fail-closed behaviour compare: every block and tx application log in a range."""
    if args.end < args.start or args.stride < 1:
        print("need end >= start and stride >= 1", file=sys.stderr)
        return 2

    heights = list(range(args.start, args.end + 1, args.stride))
    journal_path = _applog_journal_path(args)
    rows = _load_applog_journal(journal_path)
    latest = {}
    for row in rows:
        if "height" in row:
            latest[int(row["height"])] = row
    pending = [height for height in heights if not (latest.get(height) or {}).get("ok")]

    def finish(code: int) -> int:
        report_rows = _load_applog_journal(journal_path)
        report_path, passed = _write_applog_report(args, heights, report_rows)
        print(f"Report -> {report_path}")
        if code == 0 and not passed:
            code = 1
        if code != 0:
            print(
                "FAIL: application logs not proven equal "
                f"(journal={journal_path})",
                file=sys.stderr,
            )
            return code
        print(
            f"PASS: application logs match C# for every height in "
            f"[{args.start}, {args.end}] (journal={journal_path})"
        )
        return 0

    for height in pending:
        height_missing = []
        height_mismatches = []
        compared = 0
        for attempt in range(4):
            height_missing = []
            height_mismatches = []
            compared = 0
            try:
                block = rpc_call(args.reference, "getblock", [height, True])
            except Exception as exc:  # noqa: BLE001
                height_missing.append(
                    {"height": height, "item": "block", "side": "csharp", "error": str(exc)}
                )
                block = None
            if block is not None:
                try:
                    items = [("block", block["hash"])] + [
                        ("tx", tx["hash"]) for tx in block.get("tx", [])
                    ]
                except (KeyError, TypeError) as exc:
                    height_missing.append(
                        {
                            "height": height,
                            "item": "block",
                            "side": "csharp",
                            "error": f"getblock payload missing hash: {exc}",
                        }
                    )
                    items = []
                for kind, item_hash in items:
                    logs = {}
                    for side, url in (("csharp", args.reference), ("rust", args.local)):
                        try:
                            logs[side] = rpc_call(url, "getapplicationlog", [item_hash])
                        except Exception as exc:  # noqa: BLE001
                            height_missing.append(
                                {
                                    "height": height,
                                    "item": item_hash,
                                    "side": side,
                                    "error": str(exc),
                                }
                            )
                    if len(logs) != 2:
                        print(f"MISSING {height} {kind} {item_hash}", file=sys.stderr)
                        continue
                    try:
                        diffs = diff_applog(logs["rust"], logs["csharp"])
                    except ValueError as exc:
                        diffs = [str(exc)]
                    compared += 1
                    if diffs:
                        height_mismatches.append(
                            {
                                "height": height,
                                "kind": kind,
                                "item": item_hash,
                                "diffs": diffs,
                            }
                        )
                        print(
                            f"MISMATCH {height} {kind} {item_hash}: {diffs[0]}",
                            file=sys.stderr,
                        )
            if not applog_height_should_retry(height_mismatches, height_missing, attempt, 4):
                break
            print(
                f"RETRY height {height} attempt {attempt + 2} after fetch failure",
                file=sys.stderr,
                flush=True,
            )
            time.sleep(2**attempt)
        ok = not height_missing and not height_mismatches and compared > 0
        _append_applog_journal(
            journal_path,
            {
                "height": height,
                "ok": ok,
                "compared": compared,
                "mismatches": height_mismatches,
                "missing": height_missing,
            },
        )
        if args.verbose or height == heights[-1] or (height - args.start) % (args.stride * 100) == 0:
            print(
                f"height {height}: ok={ok} compared={compared} "
                f"mismatches={len(height_mismatches)} missing={len(height_missing)}",
                flush=True,
            )
        if not ok and not args.collect_all:
            return finish(1)
        if args.delay_ms > 0:
            time.sleep(args.delay_ms / 1000.0)

    return finish(0)


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

    p_al = sub.add_parser(
        "compare-applogs",
        help="Fail-closed Rust vs C# getapplicationlog compare for blocks and txs",
    )
    p_al.add_argument("--local", required=True, help="neo-rs RPC URL (ApplicationLogs enabled)")
    p_al.add_argument("--reference", default="http://seed1.neo.org:10332")
    p_al.add_argument("--start", type=int, required=True)
    p_al.add_argument("--end", type=int, required=True)
    p_al.add_argument("--stride", type=int, default=1)
    p_al.add_argument(
        "--delay-ms",
        type=int,
        default=0,
        help="Pause between heights so the public C# RPC is not flooded",
    )
    p_al.add_argument(
        "--journal",
        default="",
        help="JSONL of per-height results. A later run skips heights already marked ok. "
        "A pass requires every height in the range to be ok in this file.",
    )
    p_al.add_argument(
        "--collect-all",
        action="store_true",
        help="Keep scanning after the first missing log or field mismatch",
    )
    p_al.add_argument("--verbose", action="store_true")
    p_al.set_defaults(func=cmd_compare_applogs)

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
