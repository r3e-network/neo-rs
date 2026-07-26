#!/usr/bin/env python3
"""Compare a local neo-rs node's state roots against a reference JSONL ladder.

Reads `{"height": int, "root": "0x..."}` records (as produced by
`fetch-reference-stateroots-strided.py`), queries the local node's
`getstateroot` for each height, and reports every mismatch with the lowest
diverging height first — that height is what a divergence investigation needs.

Exit status: 0 when every compared height matched, 1 on any mismatch, 2 when
the local node could not answer for one or more heights.
"""
import argparse
import gzip
import json
import os
import sys
import threading
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

_lock = threading.Lock()


def rpc(url, method, params, timeout=20, retries=3):
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    ).encode()
    for attempt in range(retries):
        try:
            req = urllib.request.Request(
                url,
                data=payload,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            # Localhost must bypass the shell proxy: this shell's no_proxy uses
            # CIDR, which urllib cannot match, so a proxied local call returns a
            # misleading 502.
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with opener.open(req, timeout=timeout) as resp:
                raw = resp.read()
            if raw.startswith(b"\x1f\x8b"):
                raw = gzip.decompress(raw)
            body = json.loads(raw.decode())
            if "error" in body and body["error"]:
                return None, str(body["error"])
            return body.get("result"), None
        except Exception as exc:  # noqa: BLE001 - report and retry
            if attempt == retries - 1:
                return None, f"{type(exc).__name__}: {exc}"
            time.sleep(1 + attempt)
    return None, "exhausted retries"


def load_reference(path, start, end):
    refs = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            height = rec.get("height")
            root = rec.get("root") or rec.get("roothash")
            if not isinstance(height, int) or not isinstance(root, str):
                continue
            if height < start or (end is not None and height > end):
                continue
            refs[height] = root.lower()
    return refs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--local", default="http://127.0.0.1:10332")
    ap.add_argument("--reference-file", required=True)
    ap.add_argument("--start", type=int, default=0)
    ap.add_argument("--end", type=int, default=None)
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--status-file")
    ap.add_argument("--progress-every", type=int, default=500)
    args = ap.parse_args()

    refs = load_reference(args.reference_file, args.start, args.end)
    heights = sorted(refs)
    if not heights:
        print("no reference heights in range", file=sys.stderr)
        return 2

    local_height, err = rpc(args.local, "getblockcount", [])
    if err:
        print(f"local getblockcount failed: {err}", file=sys.stderr)
        return 2
    local_tip = int(local_height) - 1
    comparable = [h for h in heights if h <= local_tip]
    skipped_ahead = len(heights) - len(comparable)
    print(
        f"local_tip={local_tip} reference_heights={len(heights)} "
        f"comparable={len(comparable)} beyond_local_tip={skipped_ahead}",
        flush=True,
    )

    matched = []
    mismatched = []
    errors = []
    done = [0]

    def task(height):
        result, err = rpc(args.local, "getstateroot", [height])
        with _lock:
            done[0] += 1
            if err or not result:
                errors.append({"height": height, "error": err or "empty result"})
            else:
                got = str(result.get("roothash", "")).lower()
                if got == refs[height]:
                    matched.append(height)
                else:
                    mismatched.append(
                        {"height": height, "local": got, "reference": refs[height]}
                    )
            if done[0] % args.progress_every == 0:
                print(
                    f"progress {done[0]}/{len(comparable)} "
                    f"matched={len(matched)} mismatched={len(mismatched)} "
                    f"errors={len(errors)}",
                    flush=True,
                )

    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        list(ex.map(task, comparable))

    mismatched.sort(key=lambda m: m["height"])
    errors.sort(key=lambda e: e["height"])
    status = {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "local": args.local,
        "reference_file": os.path.abspath(args.reference_file),
        "local_tip": local_tip,
        "compared": len(comparable),
        "matched": len(matched),
        "mismatched": len(mismatched),
        "errors": len(errors),
        "beyond_local_tip": skipped_ahead,
        "lowest_match": min(matched) if matched else None,
        "highest_match": max(matched) if matched else None,
        "first_mismatch": mismatched[0] if mismatched else None,
        "mismatches": mismatched[:50],
        "error_samples": errors[:20],
    }
    print(json.dumps(status, indent=2), flush=True)
    if args.status_file:
        with open(args.status_file, "w", encoding="utf-8") as fh:
            json.dump(status, fh, indent=2)

    if mismatched:
        return 1
    if errors:
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
