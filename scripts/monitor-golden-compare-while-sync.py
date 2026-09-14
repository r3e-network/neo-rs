#!/usr/bin/env python3
"""While syncing: periodically run compare-goldens and upsert STATUS live tip."""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
VERIFY = REPO / "scripts" / "verify-protocol-consistency.py"
STATUS = REPO / "docs" / "protocol-consistency" / "STATUS.md"
LIVE = "## Live sync verification"


def tip(local: str) -> int | None:
    import gzip
    import json
    import urllib.request

    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": "getblockcount", "params": []}
    ).encode()
    req = urllib.request.Request(
        local,
        data=payload,
        headers={"Content-Type": "application/json", "Accept-Encoding": "identity"},
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            raw = resp.read()
        if raw.startswith(b"\x1f\x8b"):
            raw = gzip.decompress(raw)
        return int(json.loads(raw.decode())["result"]) - 1
    except Exception:
        return None


def upsert_live(local_height: int, note: str) -> None:
    now = datetime.now(timezone.utc).isoformat()
    live = f"""{LIVE}

Updated: {now}

- Local neo-rs tip (approx): `{local_height}`
- Note: {note}

Auto-updated by `scripts/monitor-golden-compare-while-sync.py`.
"""
    text = STATUS.read_text(encoding="utf-8") if STATUS.exists() else "# Protocol consistency status\n\n"
    if LIVE in text:
        before, _, rest = text.partition(LIVE)
        nxt = rest.find("\n## ")
        after = rest[nxt + 1 :] if nxt >= 0 else ""
        text = before.rstrip() + "\n\n" + live.rstrip() + ("\n\n" + after.lstrip() if after else "\n")
    else:
        text = text.rstrip() + "\n\n" + live.rstrip() + "\n"
    lines = []
    stamped = False
    for line in text.splitlines(keepends=True):
        if line.startswith("Updated:") and not stamped:
            lines.append(f"Updated: {now}\n")
            stamped = True
        else:
            lines.append(line)
    STATUS.write_text("".join(lines), encoding="utf-8")


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--local", default="http://127.0.0.1:10332")
    p.add_argument("--poll-seconds", type=int, default=180)
    p.add_argument("--once", action="store_true")
    args = p.parse_args()

    while True:
        t = tip(args.local)
        if t is None:
            print("waiting for local RPC...", flush=True)
            upsert_live(-1, "local RPC not ready")
            if args.once:
                return 2
            time.sleep(args.poll_seconds)
            continue

        print(f"local_tip={t} running compare-goldens", flush=True)
        proc = subprocess.run(
            [sys.executable, str(VERIFY), "compare-goldens", "--local", args.local],
            cwd=str(REPO),
            capture_output=True,
            text=True,
        )
        note = (proc.stdout or "").strip().splitlines()[-1] if proc.stdout else "no output"
        if proc.returncode != 0:
            note = f"GOLDEN_COMPARE_FAIL rc={proc.returncode}: {(proc.stderr or note)[:400]}"
            print(note, flush=True)
            upsert_live(t, note)
            return 1
        print(note, flush=True)
        upsert_live(t, note)
        if args.once:
            return 0
        time.sleep(args.poll_seconds)


if __name__ == "__main__":
    raise SystemExit(main())
