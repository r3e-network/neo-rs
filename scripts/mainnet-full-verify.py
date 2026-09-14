#!/usr/bin/env python3
"""Fail-closed MainNet state-root verifier (neo-rs vs C# reference).

Compares ``getstateroot`` for EVERY height in ``[start, end]`` between the local
neo-rs node and one or more C# reference nodes (defaults to
``seed1.neo.org:10332`` .. ``seed5.neo.org:10332``).

Contract
--------
* Fail-closed: if ANY height is missing on either side, or any root differs, the
  process exits with a non-zero status (exactly ``1`` for verification failure,
  ``2`` for usage/config errors). A green run means every height in the range
  matched.
* Resumable: matched heights are recorded in a checkpoint file so an interrupted
  campaign restarts from where it stopped. Checkpointed heights are trusted and
  skipped on resume (they were proven equal); every non-checkpointed height is
  re-fetched fresh.
* Parallel + connection-reusing: heights are fetched with a bounded thread pool,
  and each worker holds ONE persistent HTTP (keep-alive) connection per endpoint.
  Opening a fresh TCP connection per request exhausts the Windows ephemeral-port
  range (WinError 10048 / TIME_WAIT), which both destroys throughput and, worse,
  surfaces as spurious "missing" roots that abort a fail-closed run. Reference
  calls are fanned out across all configured endpoints.
* Auditable: a JSON report is written under ``--report-dir`` even on failure.
* Sampling: ``--stride N`` checks only ``start, start+N, start+2N, ...``. A run
  with ``stride > 1`` is marked ``sampled`` in the report and NEVER emits a
  ``claimable_contiguous`` claim -- a sampled sweep proves those sampled heights
  only, not the unsampled gaps, so claiming a contiguous range would be an
  overclaim. ``claim_type`` is ``"contiguous"`` (stride 1) or ``"sampled"``.

A TEST-ONLY flag ``--force-mismatch-at`` deliberately corrupts the local root at
one height; it exists solely to prove the verifier actually fails on divergence
(guarding against an always-green false positive). It must never be used in a
claiming run.
"""

from __future__ import annotations

import argparse
import gzip
import http.client
import json
import sys
import threading
import time
import urllib.parse
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]

# A value that can never collide with a real root; used by the negative self-test.
SENTINEL_ROOT = "0x" + "de" * 32

# Official N3 MainNet seeds; all serve getstateroot with identical roots.
DEFAULT_REFERENCE_SEEDS = [f"http://seed{i}.neo.org:10332" for i in range(1, 6)]


class PooledRpcClient:
    """JSON-RPC client with one persistent (keep-alive) connection per thread.

    A single instance is shared by all worker threads; each thread lazily opens
    its own socket. Connections are reused across calls and transparently
    re-established after any transport failure, so the client survives server
    ``Connection: close`` and transient resets without leaking sockets.
    """

    def __init__(self, url: str, timeout: float = 30.0) -> None:
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme not in ("http", "https"):
            raise ValueError(f"unsupported RPC scheme in {url!r}")
        self.url = url
        self.scheme = parsed.scheme
        self.host = parsed.hostname or "127.0.0.1"
        self.port = parsed.port or (443 if parsed.scheme == "https" else 80)
        self.path = parsed.path or "/"
        self.timeout = timeout
        self._tls = threading.local()

    def _connection(self) -> http.client.HTTPConnection:
        conn = getattr(self._tls, "conn", None)
        if conn is None:
            if self.scheme == "https":
                conn = http.client.HTTPSConnection(
                    self.host, self.port, timeout=self.timeout
                )
            else:
                conn = http.client.HTTPConnection(
                    self.host, self.port, timeout=self.timeout
                )
            self._tls.conn = conn
        return conn

    def _drop(self) -> None:
        conn = getattr(self._tls, "conn", None)
        if conn is not None:
            try:
                conn.close()
            except Exception:  # noqa: BLE001 - best-effort close
                pass
            self._tls.conn = None

    def call(self, method: str, params: list, retries: int = 6) -> object:
        """POST a JSON-RPC request, reusing the thread's keep-alive connection."""
        body = json.dumps(
            {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        ).encode("utf-8")
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json",
            "Accept-Encoding": "identity",
            "Connection": "keep-alive",
            "User-Agent": "neo-rs-mainnet-full-verify/1.1",
        }
        last_err: Exception | None = None
        for attempt in range(retries):
            try:
                conn = self._connection()
                conn.request("POST", self.path, body=body, headers=headers)
                resp = conn.getresponse()
                status = resp.status
                raw = resp.read()
                if status != 200:
                    # A connection-scoped failure: drop the socket and retry.
                    self._drop()
                    raise RuntimeError(
                        f"HTTP {status} from {self.url}: {raw[:200]!r}"
                    )
                if raw.startswith(b"\x1f\x8b"):
                    raw = gzip.decompress(raw)
                obj = json.loads(raw.decode("utf-8"))
                if obj.get("error"):
                    err = obj["error"]
                    code = err.get("code") if isinstance(err, dict) else None
                    if code in TRANSIENT_RPC_CODES:
                        # Server is healthy but throttling (e.g. -32001 "Too many
                        # requests"). This is NOT a missing root — retry with
                        # backoff so rate limiting never masquerades as fail-closed.
                        raise _RetryableJsonRpcError(
                            f"{method} throttled by {self.url}: {err}"
                        )
                    # The server answered; retrying a permanent error (e.g.
                    # -106 "Unknown state root") is pointless, so fail fast.
                    raise _JsonRpcError(
                        f"{method} error from {self.url}: {obj['error']}"
                    )
                return obj["result"]
            except _JsonRpcError:
                raise
            except _RetryableJsonRpcError as exc:
                last_err = exc
                time.sleep(0.5 * (attempt + 1))
            except Exception as exc:  # noqa: BLE001 - transport: fail closed, retry
                last_err = exc
                self._drop()
                time.sleep(0.4 * (attempt + 1))
        raise RuntimeError(f"{method} failed against {self.url}: {last_err}")

    def close(self) -> None:
        self._drop()


class _JsonRpcError(RuntimeError):
    """A well-formed, PERMANENT JSON-RPC error response (not a transport failure)."""


class _RetryableJsonRpcError(RuntimeError):
    """A well-formed but TRANSIENT JSON-RPC error (e.g. server-side throttling)."""


# JSON-RPC error codes that indicate a transient, server-side condition that a
# retry can clear (-32001 = "Too many requests"). These must never be recorded
# as a missing/mismatched root, or fail-closed reporting gives false negatives.
TRANSIENT_RPC_CODES = {-32001}


def normalize_root(value: str | None) -> str | None:
    if value is None:
        return None
    text = str(value).strip().lower()
    if not text.startswith("0x"):
        text = "0x" + text
    return text


def fetch_stateroot(client: PooledRpcClient, height: int, retries: int) -> dict:
    result = client.call("getstateroot", [height], retries=retries)
    if not isinstance(result, dict):
        raise RuntimeError(f"getstateroot({height}) returned non-object: {result!r}")
    root = normalize_root(result.get("roothash") or result.get("rootHash"))
    if not root:
        raise RuntimeError(f"getstateroot({height}) missing roothash/rootHash")
    return {
        "height": int(result.get("index", height)),
        "version": int(result.get("version", 0)),
        "roothash": root,
        "source": client.url,
    }


class Checkpoint:
    """Append-only record of heights proven equal, flushed atomically."""

    def __init__(self, path: Path, reference: str) -> None:
        self.path = path
        self.reference = reference
        self._lock = threading.Lock()
        self.heights: dict[int, str] = {}
        if path.exists():
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
                if data.get("reference") == reference:
                    self.heights = {
                        int(k): normalize_root(v)  # type: ignore[arg-type]
                        for k, v in (data.get("heights") or {}).items()
                    }
                else:
                    print(
                        f"checkpoint reference {data.get('reference')!r} != {reference!r}; "
                        "ignoring checkpoint",
                        file=sys.stderr,
                    )
            except (OSError, ValueError) as exc:  # noqa: BLE001
                print(f"ignoring unreadable checkpoint {path}: {exc}", file=sys.stderr)

    def add(self, height: int, root: str) -> None:
        with self._lock:
            self.heights[height] = root
            self._flush_locked()

    def _flush_locked(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "updated_at": datetime.now(timezone.utc).isoformat(),
            "reference": self.reference,
            "count": len(self.heights),
            "heights": {str(k): v for k, v in sorted(self.heights.items())},
        }
        tmp = self.path.with_suffix(self.path.suffix + ".tmp")
        tmp.write_text(json.dumps(payload), encoding="utf-8")
        tmp.replace(self.path)


def _verify_height(
    height: int,
    local_client: PooledRpcClient,
    ref_clients: list[PooledRpcClient],
    retries: int,
    force_mismatch_at: int | None,
) -> dict:
    """Return a per-height result. Never raises; encodes failures explicitly."""
    row: dict = {"height": height}
    # Deterministic primary endpoint (fan-out), then fall back to the other
    # reference endpoints so one flaky seed cannot produce a false "missing
    # reference" that needlessly fails a fail-closed run.
    n = len(ref_clients)
    start_idx = height % n
    ref_err: Exception | None = None
    for off in range(n):
        ref_client = ref_clients[(start_idx + off) % n]
        try:
            ref = fetch_stateroot(ref_client, height, retries)
            row["reference"] = ref["roothash"]
            row["reference_source"] = ref_client.url
            ref_err = None
            break
        except Exception as exc:  # noqa: BLE001
            ref_err = exc
    if ref_err is not None:
        row["status"] = "missing_reference"
        row["error"] = str(ref_err)
        return row
    try:
        loc = fetch_stateroot(local_client, height, retries)
        row["local"] = loc["roothash"]
    except Exception as exc:  # noqa: BLE001
        row["status"] = "missing_local"
        row["error"] = str(exc)
        return row

    if force_mismatch_at is not None and height == force_mismatch_at:
        row["local"] = SENTINEL_ROOT
        row["injected"] = True

    row["status"] = "match" if row["local"] == row["reference"] else "mismatch"
    return row


def _parse_reference_urls(raw: list[str] | None) -> list[str]:
    """Accept repeated flags and/or comma-separated lists; default to all seeds."""
    if not raw:
        return list(DEFAULT_REFERENCE_SEEDS)
    urls: list[str] = []
    for item in raw:
        urls.extend(part.strip() for part in item.split(",") if part.strip())
    return urls or list(DEFAULT_REFERENCE_SEEDS)


def run(args: argparse.Namespace) -> int:
    start, end = args.start, args.end
    if end < start:
        print("end must be >= start", file=sys.stderr)
        return 2
    if args.stride < 1:
        print("--stride must be >= 1", file=sys.stderr)
        return 2

    stride = args.stride
    sampled = stride > 1
    span = end - start + 1
    sample_heights = list(range(start, end + 1, stride))
    sample_count = len(sample_heights)

    # A forced mismatch must land on a sampled height; otherwise the negative
    # control would silently no-op and we could ship an always-green verifier.
    if args.force_mismatch_at is not None:
        fm = args.force_mismatch_at
        if fm < start or fm > end or (fm - start) % stride != 0:
            print(
                f"--force-mismatch-at {fm} is not a sampled height for "
                f"[{start}, {end}] (stride {stride}); refusing to run",
                file=sys.stderr,
            )
            return 2

    reference_urls = _parse_reference_urls(args.reference)
    # Key the checkpoint on the SORTED endpoint set so re-ordering --reference
    # does not needlessly invalidate an otherwise-compatible checkpoint.
    reference_label = ",".join(sorted(reference_urls))

    checkpoint = (
        Checkpoint(Path(args.checkpoint), reference_label) if args.checkpoint else None
    )

    try:
        local_client = PooledRpcClient(args.local, timeout=args.timeout)
        ref_clients = [
            PooledRpcClient(url, timeout=args.timeout) for url in reference_urls
        ]
    except ValueError as exc:
        print(f"invalid RPC url: {exc}", file=sys.stderr)
        return 2

    # Identify the heights that still need work (resume support). Under a stride
    # only the sampled heights are considered at all.
    todo: list[int] = []
    resumed = 0
    for h in sample_heights:
        if checkpoint is not None and h in checkpoint.heights:
            resumed += 1
            continue
        todo.append(h)

    print(
        f"verify plan: [{start}, {end}] stride={stride} "
        f"{'SAMPLED' if sampled else 'FULL'} span={span} sample={sample_count} "
        f"resumed={resumed} todo={len(todo)} parallel={args.parallel} "
        f"references={len(ref_clients)}",
        file=sys.stderr,
    )

    matched = resumed
    mismatches: list[dict] = []
    missing_local: list[dict] = []
    missing_reference: list[dict] = []
    processed = 0
    started = time.time()

    try:
        with ThreadPoolExecutor(max_workers=args.parallel) as pool:
            futures = {
                pool.submit(
                    _verify_height,
                    h,
                    local_client,
                    ref_clients,
                    args.retries,
                    args.force_mismatch_at,
                ): h
                for h in todo
            }
            for fut in as_completed(futures):
                row = fut.result()
                processed += 1
                status = row["status"]
                if status == "match":
                    matched += 1
                    if checkpoint is not None:
                        checkpoint.add(row["height"], row["local"])
                elif status == "mismatch":
                    mismatches.append(
                        {
                            "height": row["height"],
                            "local": row["local"],
                            "reference": row["reference"],
                            "injected": bool(row.get("injected")),
                        }
                    )
                    print(
                        f"MISMATCH {row['height']}\tlocal={row['local']}\t"
                        f"reference={row['reference']}",
                        file=sys.stderr,
                    )
                elif status == "missing_local":
                    missing_local.append({"height": row["height"], "error": row["error"]})
                    print(f"LOCAL-MISS {row['height']}\t{row['error']}", file=sys.stderr)
                elif status == "missing_reference":
                    missing_reference.append(
                        {"height": row["height"], "error": row["error"]}
                    )
                    print(f"REF-MISS {row['height']}\t{row['error']}", file=sys.stderr)

                if args.progress_every and processed % args.progress_every == 0:
                    rate = processed / max(time.time() - started, 1e-9)
                    print(
                        f"progress {processed}/{len(todo)} "
                        f"({rate:.1f} heights/s, matched={matched})",
                        file=sys.stderr,
                    )
    finally:
        local_client.close()
        for client in ref_clients:
            client.close()

    bad = (
        {m["height"] for m in mismatches}
        | {m["height"] for m in missing_local}
        | {m["height"] for m in missing_reference}
    )

    # A contiguous claim is ONLY valid for a full (stride == 1) sweep. Under a
    # stride the gaps between sampled heights were never checked, so emitting a
    # contiguous claim would be an overclaim; we refuse to do so.
    if not sampled:
        claim_end = start - 1
        for h in range(start, end + 1):
            if h in bad:
                break
            claim_end = h
        claimable = (
            {"start": start, "end": claim_end} if claim_end >= start else None
        )
        passed = not bad and claim_end == end
        claim_type = "contiguous"
    else:
        claimable = None  # sampled runs make no contiguous claim
        passed = not bad and matched == sample_count
        claim_type = "sampled"

    elapsed = time.time() - started
    elapsed_secs = round(elapsed, 3)
    hps = round(processed / elapsed, 2) if elapsed > 0 else 0.0
    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "local": args.local,
        "reference": reference_label,
        "reference_urls": reference_urls,
        "start": start,
        "end": end,
        "stride": stride,
        "sampled": sampled,
        "claim_type": claim_type,
        "span": span,
        "sample_count": sample_count,
        "total": sample_count,
        "matched": matched,
        "processed_this_run": processed,
        "resumed_from_checkpoint": resumed,
        "mismatches": mismatches,
        "missing_local": missing_local,
        "missing_reference": missing_reference,
        "claimable_contiguous": claimable,
        "forced_mismatch_at": args.force_mismatch_at,
        "passed": passed,
        "elapsed_secs": elapsed_secs,
        "heights_per_sec": hps,
    }

    report_dir = Path(args.report_dir)
    report_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    report_path = report_dir / f"mainnet-full-verify-{start}-{end}-{stamp}.json"
    if args.report:
        report_path = Path(args.report)
        report_path.parent.mkdir(parents=True, exist_ok=True)
    report["report_path"] = str(report_path)
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Report -> {report_path}")

    if not passed:
        scope = "sampled set" if sampled else "full range"
        print(
            f"FAIL: state-root verification not proven for the {scope} "
            f"(matched={matched}, mismatches={len(mismatches)}, "
            f"missing_local={len(missing_local)}, "
            f"missing_reference={len(missing_reference)})",
            file=sys.stderr,
        )
        if report["claimable_contiguous"]:
            c = report["claimable_contiguous"]
            print(
                f"Partial claimable contiguous only: [{c['start']}, {c['end']}]",
                file=sys.stderr,
            )
        return 1

    if sampled:
        print(
            f"PASS (SAMPLED stride={stride}): {matched}/{sample_count} sampled "
            f"heights in [{start}, {end}] matched C# state roots "
            f"({hps} heights/s, {elapsed_secs}s). NOT a contiguous proof: the "
            f"{span - sample_count} unsampled heights were not checked."
        )
    else:
        print(
            f"PASS: contiguous [{start}, {end}] matched C# state roots "
            f"({matched} heights, {hps} heights/s, {elapsed_secs}s)"
        )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--local", required=True, help="local neo-rs RPC URL")
    parser.add_argument(
        "--reference",
        action="append",
        default=None,
        help=(
            "C# reference RPC URL(s). Repeatable and/or comma-separated. "
            "Defaults to seed1..seed5.neo.org:10332 (kept-alive fan-out)."
        ),
    )
    parser.add_argument("--start", type=int, default=0)
    parser.add_argument("--end", type=int, required=True)
    parser.add_argument(
        "--stride",
        type=int,
        default=1,
        help=(
            "sample every Nth height (start, start+N, start+2N, ...). Default 1 "
            "= every height. stride>1 marks the run SAMPLED and disables any "
            "contiguous claim in the report."
        ),
    )
    parser.add_argument(
        "--parallel",
        type=int,
        default=64,
        help="worker threads (keep-alive; scales near-linearly, no socket exhaustion)",
    )
    parser.add_argument("--timeout", type=float, default=30.0, help="per-call timeout (s)")
    parser.add_argument("--retries", type=int, default=6, help="per-call retries")
    parser.add_argument(
        "--checkpoint",
        default=None,
        help="path to resumable checkpoint JSON (omit to disable resume)",
    )
    parser.add_argument("--report-dir", default="outputs", help="where to write the report")
    parser.add_argument("--report", default=None, help="explicit report file path")
    parser.add_argument(
        "--progress-every", type=int, default=500, help="progress log interval (heights)"
    )
    parser.add_argument(
        "--force-mismatch-at",
        type=int,
        default=None,
        help=(
            "TEST ONLY: corrupt the local root at this height to prove "
            "fail-closed. Under --stride it must be a sampled height, else the "
            "run exits 2 (so the negative control can never silently no-op)."
        ),
    )
    return parser


def main() -> int:
    return run(build_parser().parse_args())


if __name__ == "__main__":
    sys.exit(main())
