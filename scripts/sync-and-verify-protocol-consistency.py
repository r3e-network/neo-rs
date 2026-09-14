#!/usr/bin/env python3
"""
While neo-rs syncs MainNet, periodically fail-closed-compare contiguous
state roots against C# seeds and record the claimable [0, H].

Usage:
  python scripts/sync-and-verify-protocol-consistency.py
  python scripts/sync-and-verify-protocol-consistency.py --once
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
STATUS = REPO / "docs" / "protocol-consistency" / "STATUS.md"
REPORT_DIR = REPO / "docs" / "protocol-consistency" / "reports"
VERIFY = REPO / "scripts" / "verify-protocol-consistency.py"


def rpc_height(url: str) -> int | None:
    try:
        out = subprocess.check_output(
            [
                sys.executable,
                str(VERIFY),
                "compare-rpc",
                "--help",
            ],
            text=True,
            stderr=subprocess.STDOUT,
        )
        del out
    except Exception:
        pass

    import gzip
    import urllib.request

    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": "getblockcount", "params": []}
    ).encode()
    req = urllib.request.Request(
        url,
        data=payload,
        headers={
            "Content-Type": "application/json",
            "Accept-Encoding": "identity",
            "User-Agent": "neo-rs-sync-verify/1.0",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            raw = resp.read()
        if raw.startswith(b"\x1f\x8b"):
            raw = gzip.decompress(raw)
        body = json.loads(raw.decode())
        if "error" in body:
            return None
        # Neo getblockcount returns tip+1
        count = int(body["result"])
        return max(count - 1, 0)
    except Exception:
        return None


def run_compare(local: str, reference: str, end: int) -> tuple[int, Path | None]:
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    cmd = [
        sys.executable,
        str(VERIFY),
        "compare-rpc",
        "--local",
        local,
        "--reference",
        reference,
        "--start",
        "0",
        "--end",
        str(end),
    ]
    proc = subprocess.run(cmd, cwd=str(REPO), capture_output=True, text=True)
    # Find newest report
    reports = sorted(REPORT_DIR.glob(f"compare-rpc-0-{end}-*.json"))
    report = reports[-1] if reports else None
    if proc.returncode != 0:
        print(proc.stdout)
        print(proc.stderr, file=sys.stderr)
    else:
        print(proc.stdout)
    return proc.returncode, report


LIVE_SECTION_MARKER = "## Live sync verification"


def update_status(local_height: int, claim_end: int | None, note: str) -> None:
    """Upsert the Live sync section without wiping known divergences / policy."""
    now = datetime.now(timezone.utc).isoformat()
    claim = (
        f"`[{0}, {claim_end}]`"
        if claim_end is not None and claim_end >= 0
        else "**none yet**"
    )
    live = f"""{LIVE_SECTION_MARKER}

Updated: {now}

- Local neo-rs tip (approx): `{local_height}`
- Claimable contiguous fail-closed match vs C#: {claim}
- Note: {note}

Auto-updated by `scripts/sync-and-verify-protocol-consistency.py` (does not rewrite
other sections). Authoritative policy: `docs/PROTOCOL_CONSISTENCY.md`.
"""
    if STATUS.exists():
        text = STATUS.read_text(encoding="utf-8")
    else:
        text = "# Protocol consistency status\n\n"

    if LIVE_SECTION_MARKER in text:
        before, _, rest = text.partition(LIVE_SECTION_MARKER)
        # Drop previous live section through the next ## heading (or EOF).
        next_heading = rest.find("\n## ")
        if next_heading >= 0:
            after = rest[next_heading + 1 :]  # keep leading ## of next section
            text = before.rstrip() + "\n\n" + live.rstrip() + "\n\n" + after.lstrip()
        else:
            text = before.rstrip() + "\n\n" + live.rstrip() + "\n"
    else:
        # Insert after title / first paragraph block.
        lines = text.splitlines(keepends=True)
        insert_at = 0
        for i, line in enumerate(lines):
            if line.startswith("## "):
                insert_at = i
                break
        else:
            insert_at = len(lines)
        lines.insert(insert_at, "\n" + live.rstrip() + "\n\n")
        text = "".join(lines)

    # Keep a top-level Updated: stamp in sync with the live section.
    stamped = []
    saw_updated = False
    for line in text.splitlines(keepends=True):
        if line.startswith("Updated:") and not saw_updated:
            stamped.append(f"Updated: {now}\n")
            saw_updated = True
        else:
            stamped.append(line)
    STATUS.write_text("".join(stamped), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--local", default="http://127.0.0.1:10332")
    parser.add_argument("--reference", default="http://seed1.neo.org:10332")
    parser.add_argument("--poll-seconds", type=int, default=120)
    parser.add_argument(
        "--chunk",
        type=int,
        default=500,
        help="Compare in growing end heights; each pass compares [0, min(tip, last+chunk)]",
    )
    parser.add_argument("--once", action="store_true")
    parser.add_argument(
        "--max-end",
        type=int,
        default=None,
        help="Optional cap for compare end height during early sync",
    )
    args = parser.parse_args()

    last_compared_end = -1
    while True:
        tip = rpc_height(args.local)
        if tip is None:
            print("waiting for local RPC...", flush=True)
            update_status(-1, None, "local RPC not ready")
            if args.once:
                return 2
            time.sleep(args.poll_seconds)
            continue

        target_end = tip
        if args.max_end is not None:
            target_end = min(target_end, args.max_end)
        # Grow compared window gradually to find earliest mismatch faster.
        if last_compared_end < 0:
            end = min(target_end, args.chunk)
        else:
            end = min(target_end, last_compared_end + args.chunk)
        if end < 0:
            end = 0

        print(f"local_tip={tip} comparing [0, {end}]", flush=True)
        code, report = run_compare(args.local, args.reference, end)
        claim_end = None
        note = "compare failed"
        if report and report.exists():
            data = json.loads(report.read_text(encoding="utf-8"))
            claim = data.get("claimable_contiguous")
            if claim:
                claim_end = int(claim["end"])
            mismatches = data.get("mismatches") or []
            if mismatches:
                first = mismatches[0]["height"]
                note = f"FIRST_MISMATCH height={first} report={report.name}"
                print(note, flush=True)
                update_status(tip, claim_end, note)
                # Stop growing past the first mismatch — this is the work queue.
                return 1
            if code == 0:
                note = f"PASS contiguous [0, {end}] report={report.name}"
                last_compared_end = end
            else:
                note = f"incomplete compare report={report.name} (missing roots?)"
                # Still advance carefully using claimable prefix
                if claim_end is not None:
                    last_compared_end = claim_end
        update_status(tip, claim_end if claim_end is not None else (end if code == 0 else None), note)

        if args.once:
            return code
        if tip >= end and code == 0 and end >= tip:
            # Fully caught up for now; idle until tip grows.
            time.sleep(args.poll_seconds)
            continue
        time.sleep(max(5, args.poll_seconds // 4))


if __name__ == "__main__":
    sys.exit(main())
