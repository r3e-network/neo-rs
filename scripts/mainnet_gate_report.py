#!/usr/bin/env python3
"""Gate-based campaign reporter for the MainNet in-import state-root gate.

The dense verification lane does NOT sweep RPC roots. Instead the importer runs
with an explicit, required reference-roots file; ``neo::state_service`` compares
the computed root against the reference at EVERY executed height and, on the
first divergence, returns an error that aborts the import. This script turns that
run into a machine-readable, fail-closed campaign report.

It has two modes:

* ``--gate-only``  parse the output of
  ``scripts/download_reference_roots.py --verify-only`` and decide PASS/FAIL.
  The gate is a *precondition*: if the reference file is not complete for the
  campaign range, the campaign must refuse to run.

* report mode (default)  parse the importer log + exit code and emit a JSON
  report under ``outputs/`` with observations and a contiguous-range claim only
  when complete clean evidence is present.

Fail-closed rules (the whole point of this tool):
  * An aborted run exits non-zero and is reported as ``diverged``/``aborted``.
  * A partial (bounded) run is reported as ``partial``; it is NEVER labelled a
    verified contiguous full range.
  * Only an import with exit code 0, an explicit clean-completion marker, no
    parsed mismatch, ``final_height == range_end``, and an observed reference
    gate load covering the campaign range is reported as ``clean`` with
    ``verified_contiguous_prefix == [start, end]``.
  * Every non-clean status has a null verified contiguous prefix. Divergence,
    final-height, and flush markers are observations, not proof of a contiguous
    prefix: a resumed importer may have skipped earlier blocks.

Exit codes: 0 = clean full pass (report mode) / gate effectively passed
(--gate-only); 1 = anything else; 2 = usage error.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

# Known-bad / historically sensitive heights: an abort AT one of these is an
# expected stop, not a surprise. 5107 = CommitteeInfoContract.setInfo (first
# divergence); 21373 = GasToken fee-burn divergence; the rest are historical
# reproducers.
DEFAULT_EXPECTED_STOPS = [5107, 21373, 980196, 1465790]

MISMATCH_RE = re.compile(
    r"state root mismatch at height (\d+): computed "
    r"(0x[0-9a-fA-F]{64}), expected (0x[0-9a-fA-F]{64}), "
    r"put_count (\d+), del_count (\d+)"
)
PERSIST_FAIL_RE = re.compile(r"failed to persist imported block (\d+)")
FINAL_HEIGHT_RE = re.compile(r'"final_height":(\d+)')
FLUSH_HEIGHT_RE = re.compile(r'"persisted_height":(\d+)')
COMPLETED_RE = re.compile(r"completed \.acc import")
REFERENCE_GATE_LOADED_RE = re.compile(
    r"loaded (\d+) reference state roots for validation covering heights "
    r"\[(\d+), (\d+)\]"
)


def _norm_root(value: str) -> str:
    text = value.strip().lower()
    if not text.startswith("0x"):
        text = "0x" + text
    return text


def parse_gate_output(text: str) -> dict:
    """Parse the human output of download_reference_roots.py --verify-only."""
    def grab(label: str) -> int | None:
        m = re.search(rf"^{re.escape(label)}\s*:\s*(\d+)", text, re.MULTILINE)
        return int(m.group(1)) if m else None

    present = grab("present")
    missing = grab("missing")
    duplicates = grab("duplicates")
    malformed = grab("malformed")
    out_of_range = grab("out-of-range")
    exists = "file exists : True" in text
    gate_pass = re.search(r"^GATE: PASS\s*$", text, re.MULTILINE) is not None
    fail_block = re.search(r"^GATE: FAIL\s*$((?:\n\s+- .*)*)", text, re.MULTILINE)
    fail_reasons = (
        [line.strip()[2:].strip() for line in fail_block.group(1).splitlines() if line.strip()]
        if fail_block
        else []
    )
    return {
        "exists": exists,
        "present": present,
        "missing": missing,
        "duplicates": duplicates,
        "malformed": malformed,
        "out_of_range": out_of_range,
        "gate_pass_exact": gate_pass,
        "gate_fail_reasons": fail_reasons,
    }


def gate_effectively_passed(rep: dict, span: int) -> bool:
    """Require explicit gate success and exact, clean campaign coverage.

    Out-of-range records (heights beyond ``range_end``) are tolerated: the node
    only compares the heights it actually executes, so extra roots are harmless.
    Every required counter must be present; missing counters never mean zero.
    """
    required_zero = ("missing", "duplicates", "malformed")
    if any(rep.get(key) is None for key in ("present", *required_zero)):
        return False
    reasons = rep.get("gate_fail_reasons") or []
    only_out_of_range = bool(reasons) and all(
        re.fullmatch(r"\d+ out-of-range heights", reason) for reason in reasons
    )
    return (
        rep.get("exists") is True
        and (rep.get("gate_pass_exact") is True or only_out_of_range)
        and rep["present"] == span
        and all(rep[key] == 0 for key in required_zero)
    )


def parse_import_log(text: str) -> dict:
    """Extract divergence, progress, and active reference-gate markers."""
    mismatches = [
        {
            "height": int(m.group(1)),
            "computed": _norm_root(m.group(2)),
            "expected": _norm_root(m.group(3)),
            "put_count": int(m.group(4)),
            "del_count": int(m.group(5)),
        }
        for m in MISMATCH_RE.finditer(text)
    ]
    mismatches.sort(key=lambda r: r["height"])

    persist_fails = [int(m.group(1)) for m in PERSIST_FAIL_RE.finditer(text)]
    final_heights = [int(m.group(1)) for m in FINAL_HEIGHT_RE.finditer(text)]
    flush_heights = [int(m.group(1)) for m in FLUSH_HEIGHT_RE.finditer(text)]
    reference_gate_loads = list(REFERENCE_GATE_LOADED_RE.finditer(text))
    reference_gate_load = reference_gate_loads[-1] if reference_gate_loads else None
    return {
        "mismatches": mismatches,
        "first_mismatch": mismatches[0] if mismatches else None,
        "persist_fail_height": max(persist_fails) if persist_fails else None,
        "final_height": max(final_heights) if final_heights else None,
        "last_flush_height": max(flush_heights) if flush_heights else None,
        "clean_completion": bool(COMPLETED_RE.search(text)),
        "reference_gate_active": reference_gate_load is not None,
        "reference_gate_count": (
            int(reference_gate_load.group(1)) if reference_gate_load else None
        ),
        "reference_gate_range": (
            [int(reference_gate_load.group(2)), int(reference_gate_load.group(3))]
            if reference_gate_load
            else None
        ),
    }


def build_report(args: argparse.Namespace) -> tuple[dict, int]:
    range_start, range_end = args.range_start, args.range_end
    if range_end < range_start:
        print("range_end must be >= range_start", file=sys.stderr)
        return {}, 2
    span = range_end - range_start + 1

    gate_text = ""
    if args.gate_output and Path(args.gate_output).exists():
        gate_text = Path(args.gate_output).read_text(encoding="utf-8", errors="replace")
    gate_rep = parse_gate_output(gate_text) if gate_text else None
    gate_ok = bool(
        args.gate_ok
        and gate_rep is not None
        and gate_effectively_passed(gate_rep, span)
    )

    import_text = ""
    if args.import_log and Path(args.import_log).exists():
        import_text = Path(args.import_log).read_text(encoding="utf-8", errors="replace")
    parsed = parse_import_log(import_text)
    reference_gate_range = parsed["reference_gate_range"]
    reference_gate_covers = bool(
        parsed["reference_gate_active"]
        and reference_gate_range is not None
        and reference_gate_range[0] <= range_start
        and reference_gate_range[1] >= range_end
    )

    expected_stops = set(args.expected_stops)

    # ---- Fail-closed classification --------------------------------------
    notes: list[str] = []
    if not gate_ok:
        status = "gate_failed"
        notes.append(
            "Precondition gate did not pass: reference file is not complete for the "
            "campaign range; the import must not be treated as evidence."
        )
        prefix = None
    elif args.import_exit_code != 0:
        first = parsed["first_mismatch"]
        if first is not None:
            status = "diverged"
            prefix = None
            if first["height"] in expected_stops:
                notes.append(
                    f"Divergence at height {first['height']} is a KNOWN expected stop."
                )
            notes.append(
                f"Import aborted at the first parsed state-root divergence (height "
                f"{first['height']}). This is an observation only: a resumed "
                f"importer may have skipped earlier blocks, so no contiguous "
                f"prefix is claimed."
            )
        else:
            status = "aborted"
            # A flush checkpoint proves persistence, not contiguous root equality.
            prefix = None
            notes.append(
                "Abort without a parsed state-root divergence; no contiguous "
                "prefix can be claimed from progress markers alone (fail-closed)."
            )
            if parsed["persist_fail_height"] is not None:
                notes.append(
                    f"Importer failed to persist block "
                    f"{parsed['persist_fail_height']} (see raw log tail)."
                )
    else:
        final_height = parsed["final_height"]
        if (
            parsed["clean_completion"]
            and final_height == range_end
            and parsed["first_mismatch"] is None
            and reference_gate_covers
        ):
            status = "clean"
            prefix = {"start": range_start, "end": range_end}
            notes.append(
                "Clean completion marker, exact final_height==range_end, zero "
                "parsed divergences, and an active reference-gate load covering "
                "the campaign range."
            )
        else:
            status = "partial"
            prefix = None
            notes.append(
                "Import exited 0 without complete clean evidence (explicit "
                "completion marker, final_height==range_end, no parsed "
                "divergence, and an observed reference-gate load covering the "
                "campaign range are all required); no prefix is claimed."
            )

    if status == "clean":
        claim = (
            f"neo-rs state roots verified contiguous [{range_start}, {range_end}] "
            f"against C# reference via the in-import gate (fail-closed)."
        )
    else:
        claim = f"no verified contiguous range (status={status})."

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "campaign": {
            "range_start": range_start,
            "range_end": range_end,
            "span": span,
            "config": args.config,
            "store": args.store,
            "acc_file": args.acc_file,
        },
        "reference": {
            "path": args.reference_file,
            "record_count": args.reference_count,
            "verified_range": [range_start, range_end],
            "gate": {
                "passed": bool(gate_ok),
                "raw": gate_rep,
                "active": parsed["reference_gate_active"],
                "count": parsed["reference_gate_count"],
                "range": parsed["reference_gate_range"],
            },
        },
        "import": {
            "log": args.import_log,
            "exit_code": args.import_exit_code,
            "clean_completion": parsed["clean_completion"],
            "final_height": parsed["final_height"],
            "last_flush_height": parsed["last_flush_height"],
        },
        "first_divergence": (
            {
                **parsed["first_mismatch"],
                "is_known_expected_stop": (
                    parsed["first_mismatch"]["height"] in expected_stops
                    if parsed["first_mismatch"]
                    else False
                ),
            }
            if parsed["first_mismatch"]
            else None
        ),
        "abort_height": parsed["persist_fail_height"],
        "expected_stops": sorted(expected_stops),
        "verified_contiguous_prefix": prefix,
        "status": status,
        "claim": claim,
        "notes": notes,
    }

    if args.log_tail_n > 0 and import_text:
        report["raw_log_tail"] = import_text.splitlines()[-args.log_tail_n :]

    return report, (0 if status == "clean" else 1)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--gate-only", action="store_true",
                   help="only evaluate the reference gate; exit 0 if effectively passed")
    p.add_argument("--gate-output", default=None, help="captured --verify-only output")
    p.add_argument("--range-start", type=int, required=True)
    p.add_argument("--range-end", type=int, required=True)
    p.add_argument("--import-log", default=None)
    p.add_argument("--import-exit-code", type=int, default=-1)
    p.add_argument("--gate-ok", type=lambda v: str(v).lower() in ("1", "true", "yes"),
                   default=True, help="whether the precondition gate passed")
    p.add_argument("--reference-file", default="data/reference_stateroots.jsonl")
    p.add_argument("--reference-count", type=int, default=None)
    p.add_argument("--config", default=None)
    p.add_argument("--store", default=None)
    p.add_argument("--acc-file", default=None)
    p.add_argument("--expected-stops", default=",".join(str(h) for h in DEFAULT_EXPECTED_STOPS))
    p.add_argument("--log-tail-n", type=int, default=40)
    p.add_argument("--out", default=None, help="report path (default: outputs/mainnet-campaign-<ts>.json)")
    args = p.parse_args(argv)

    if args.expected_stops.strip():
        args.expected_stops = [int(x) for x in args.expected_stops.split(",") if x.strip()]
    else:
        args.expected_stops = []

    span = args.range_end - args.range_start + 1
    if args.gate_only:
        text = ""
        if args.gate_output and Path(args.gate_output).exists():
            text = Path(args.gate_output).read_text(encoding="utf-8", errors="replace")
        rep = parse_gate_output(text) if text else {"present": None}
        ok = gate_effectively_passed(rep, span)
        print(json.dumps({"gate_effective_pass": ok, "span": span, "parsed": rep}))
        return 0 if ok else 1

    report, rc = build_report(args)
    if not report:
        return rc

    out = args.out
    if not out:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        out = f"outputs/mainnet-campaign-{args.range_start}-{args.range_end}-{stamp}.json"
    out_path = Path(out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    report["report_path"] = str(out_path)
    out_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Report -> {out_path}")
    print(f"status={report['status']} prefix={report['verified_contiguous_prefix']}")
    return rc


if __name__ == "__main__":
    sys.exit(main())
