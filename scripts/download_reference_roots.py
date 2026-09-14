#!/usr/bin/env python3
"""Download the full C# reference state-root set for a height range into a
resumable, crash-safe JSONL file, then prove the file is COMPLETE.

This is the production downloader used to pre-fetch every reference state root in
``[--start, --end]`` so the importer can validate each height in-import against a
complete file.  It is a NEW script and deliberately does not touch
``scripts/download_stateroots.py`` (which may be running concurrently).

Design highlights
-----------------
* **Keep-alive client.**  One persistent ``http.client.HTTPConnection`` per worker
  thread (not one connection per request).  This avoids the Windows
  ephemeral-port / TIME_WAIT exhaustion that turns the old urllib client into a
  throughput collapse and a fail-closed abort source.
* **Multi-endpoint fan-out.**  Heights are served by a pool of workers spread over
  several endpoints (default seed1..seed5.neo.org:10332), with a hard per-endpoint
  worker cap, so no single endpoint is over-driven.
* **Resumable + crash-safe.**  Append-only JSONL, one record per line, written
  with a single ``os.write`` behind a lock (no interleaved partial lines).  On
  startup the existing file is scanned and already-present heights are skipped.
* **Never silently drop a height.**  A height that still fails after all retries is
  recorded in a sidecar failure file (``<output>.failed.jsonl``) *and* is simply
  absent from the main file, so the completeness gate fails — it is never masked.
* **Completeness gate.**  ``--verify-only`` asserts the file contains exactly one
  well-formed record for every height in the range (contiguous, no duplicates,
  ``roothash`` == ``0x`` + 64 hex).  It can be re-run at any time.

Output format (exactly what ``neo::state_service`` reads):
    {"height": <int>, "roothash": "0x<64 hex>"}

Exit codes: 0 = success / gate passed; 1 = gate failed or fetch incomplete;
2 = usage error.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import queue
import re
import socket
import sys
import threading
import time

DEFAULT_SEEDS = [
    "seed1.neo.org",
    "seed2.neo.org",
    "seed3.neo.org",
    "seed4.neo.org",
    "seed5.neo.org",
]
DEFAULT_PORT = 10332
DEFAULT_OUTPUT = "data/reference_stateroots.jsonl"
DEFAULT_START = 0
DEFAULT_END = 13141250  # inclusive -> 13,141,251 heights
DEFAULT_WORKERS_PER_SEED = 16

ROOT_RE = re.compile(r"^0x[0-9a-fA-F]{64}$")
_HEX64 = re.compile(r"^[0-9a-fA-F]{64}$")


# --------------------------------------------------------------------------- #
# Endpoint client
# --------------------------------------------------------------------------- #
class SeedClient:
    """A single persistent keep-alive JSON-RPC connection to one endpoint.

    The connection is created lazily and reused across calls.  A caller that
    catches a transport error should call :meth:`close` before the next call so a
    possibly-poisoned keep-alive connection is replaced.
    """

    def __init__(self, host: str, port: int, timeout: float) -> None:
        self.host = host
        self.port = port
        self.timeout = timeout
        self._conn: http.client.HTTPConnection | None = None

    def _connect(self) -> None:
        self.close()
        self._conn = http.client.HTTPConnection(self.host, self.port, timeout=self.timeout)

    def close(self) -> None:
        if self._conn is not None:
            try:
                self._conn.close()
            except Exception:  # noqa: BLE001 - best-effort cleanup
                pass
            self._conn = None

    def get_stateroot(self, height: int) -> str:
        """Fetch ``getstateroot(height)``; return a normalised ``0x``-root string.

        Raises on any transport error, JSON-RPC error, or malformed root so the
        caller can retry.
        """
        if self._conn is None:
            self._connect()
        payload = json.dumps(
            {"jsonrpc": "2.0", "id": 1, "method": "getstateroot", "params": [height]}
        ).encode("utf-8")
        assert self._conn is not None
        self._conn.request(
            "POST",
            "/",
            body=payload,
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json",
                "Accept-Encoding": "identity",
                "User-Agent": "neo-rs-download-reference-roots/1.0",
            },
        )
        resp = self._conn.getresponse()
        raw = resp.read()  # read fully so the connection can be reused
        if resp.status != 200:
            raise RuntimeError(f"http_{resp.status}")
        data = json.loads(raw.decode("utf-8"))
        if data.get("error"):
            raise RuntimeError(f"rpc_error: {data['error']}")
        result = data.get("result")
        if not isinstance(result, dict):
            raise RuntimeError(f"non-object result: {result!r}")
        root = result.get("roothash") or result.get("rootHash")
        if root is None:
            raise RuntimeError("missing roothash")
        root = str(root).strip()
        if not root.startswith("0x") and not root.startswith("0X"):
            root = "0x" + root
        root = "0x" + root[2:].lower()
        if not ROOT_RE.match(root):
            raise RuntimeError(f"malformed roothash: {root!r}")
        return root


# --------------------------------------------------------------------------- #
# Append-only writers (crash-safe, single-write per line)
# --------------------------------------------------------------------------- #
class JsonlAppender:
    """Append JSONL records with atomic single-write lines and periodic fsync."""

    def __init__(self, path: str, fsync_every: int = 2000) -> None:
        self.path = path
        self.fsync_every = max(1, fsync_every)
        self._lock = threading.Lock()
        self._since_fsync = 0
        parent = os.path.dirname(os.path.abspath(path))
        if parent:
            os.makedirs(parent, exist_ok=True)
        # O_APPEND + single os.write -> no interleaved partial lines.
        self._fd = os.open(path, os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o644)

    def write(self, height: int, roothash: str) -> None:
        line = json.dumps({"height": int(height), "roothash": str(roothash)}) + "\n"
        blob = line.encode("utf-8")
        with self._lock:
            os.write(self._fd, blob)
            self._since_fsync += 1
            if self._since_fsync >= self.fsync_every:
                self._flush_locked()

    def write_raw(self, obj: dict) -> None:
        line = json.dumps(obj) + "\n"
        blob = line.encode("utf-8")
        with self._lock:
            os.write(self._fd, blob)
            self._since_fsync += 1
            if self._since_fsync >= self.fsync_every:
                self._flush_locked()

    def _flush_locked(self) -> None:
        try:
            os.fsync(self._fd)
        except OSError:
            pass
        self._since_fsync = 0

    def flush(self) -> None:
        with self._lock:
            self._flush_locked()

    def close(self) -> None:
        with self._lock:
            try:
                self._flush_locked()
            finally:
                try:
                    os.close(self._fd)
                except OSError:
                    pass


# --------------------------------------------------------------------------- #
# Counters / progress
# --------------------------------------------------------------------------- #
class Counters:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self.ok = 0
        self.failed = 0

    def inc_ok(self) -> None:
        with self._lock:
            self.ok += 1

    def inc_failed(self) -> None:
        with self._lock:
            self.failed += 1

    def snapshot(self) -> tuple[int, int]:
        with self._lock:
            return self.ok, self.failed


# --------------------------------------------------------------------------- #
# File scanning helpers
# --------------------------------------------------------------------------- #
def _iter_valid_lines(path: str):
    """Yield ``(lineno, dict)`` for well-formed lines; count malformed via yield too."""
    with open(path, "r", encoding="utf-8") as f:
        for lineno, raw in enumerate(f, start=1):
            s = raw.strip()
            if not s:
                continue
            try:
                yield lineno, json.loads(s)
            except Exception:  # noqa: BLE001
                yield lineno, None


def load_present_bitset(path: str, start: int, end: int):
    """Scan an existing output file.

    Returns ``(bits, present, duplicates, malformed, out_of_range)`` where ``bits``
    is a bytearray bitset indexed by ``height - start``.
    """
    span = end - start + 1
    bits = bytearray((span + 7) // 8)
    present = 0
    duplicates = 0
    malformed = 0
    out_of_range = 0
    if not os.path.exists(path):
        return bits, present, duplicates, malformed, out_of_range
    for _lineno, obj in _iter_valid_lines(path):
        if obj is None:
            malformed += 1
            continue
        try:
            h = int(obj["height"])
        except Exception:  # noqa: BLE001
            malformed += 1
            continue
        if h < start or h > end:
            out_of_range += 1
            continue
        i = h - start
        byte, mask = i >> 3, 1 << (i & 7)
        if bits[byte] & mask:
            duplicates += 1
        else:
            bits[byte] |= mask
            present += 1
    return bits, present, duplicates, malformed, out_of_range


def scan_full(path: str, start: int, end: int):
    """Full validation scan used by the completeness gate.

    Returns a dict with counts plus up to 50 example missing/malformed/duplicate
    heights and any out-of-range heights seen.
    """
    span = end - start + 1
    bits = bytearray((span + 7) // 8)
    present = 0
    duplicates = 0
    malformed_lines = 0
    bad_roots = 0
    missing_roots = 0
    out_of_range = 0
    dup_examples: list[int] = []
    bad_examples: list[str] = []
    oor_examples: list[int] = []

    if not os.path.exists(path):
        return {
            "exists": False,
            "present": 0,
            "duplicates": 0,
            "malformed_lines": 0,
            "bad_roots": 0,
            "missing_roots": 0,
            "out_of_range": 0,
            "missing_count": span,
            "missing_examples": list(range(start, min(start + 50, end + 1))),
            "dup_examples": [],
            "bad_examples": [],
            "oor_examples": [],
        }

    for lineno, obj in _iter_valid_lines(path):
        if obj is None:
            malformed_lines += 1
            if len(bad_examples) < 50:
                bad_examples.append(f"line {lineno}: not JSON")
            continue
        try:
            h = int(obj["height"])
        except Exception:  # noqa: BLE001
            malformed_lines += 1
            if len(bad_examples) < 50:
                bad_examples.append(f"line {lineno}: bad height")
            continue
        root = obj.get("roothash")
        if root is None:
            missing_roots += 1
            if len(bad_examples) < 50:
                bad_examples.append(f"line {lineno}: missing roothash (h={h})")
            continue
        root_str = str(root).strip()
        hexpart = root_str[2:] if root_str[:2].lower() == "0x" else root_str
        if not _HEX64.match(hexpart) or root_str[:2].lower() != "0x":
            bad_roots += 1
            if len(bad_examples) < 50:
                bad_examples.append(f"line {lineno}: malformed roothash (h={h})")
            # still count the height as present-if-in-range? No: it is unusable.
            continue
        if h < start or h > end:
            out_of_range += 1
            if len(oor_examples) < 50:
                oor_examples.append(h)
            continue
        i = h - start
        byte, mask = i >> 3, 1 << (i & 7)
        if bits[byte] & mask:
            duplicates += 1
            if len(dup_examples) < 50:
                dup_examples.append(h)
        else:
            bits[byte] |= mask
            present += 1

    missing_examples: list[int] = []
    missing_count = 0
    for i in range(span):
        if not (bits[i >> 3] & (1 << (i & 7))):
            missing_count += 1
            if len(missing_examples) < 50:
                missing_examples.append(start + i)

    return {
        "exists": True,
        "present": present,
        "duplicates": duplicates,
        "malformed_lines": malformed_lines,
        "bad_roots": bad_roots,
        "missing_roots": missing_roots,
        "out_of_range": out_of_range,
        "missing_count": missing_count,
        "missing_examples": missing_examples,
        "dup_examples": dup_examples,
        "bad_examples": bad_examples,
        "oor_examples": oor_examples,
    }


def count_failure_file(path: str) -> int:
    if not os.path.exists(path):
        return 0
    n = 0
    with open(path, "r", encoding="utf-8") as f:
        for raw in f:
            if raw.strip():
                n += 1
    return n


# --------------------------------------------------------------------------- #
# Completeness gate
# --------------------------------------------------------------------------- #
def run_gate(output: str, start: int, end: int, failed_path: str) -> bool:
    span = end - start + 1
    rep = scan_full(output, start, end)
    failures = count_failure_file(failed_path)

    print("\n=== COMPLETENESS GATE ===")
    print(f"output      : {output}")
    print(f"range       : [{start}, {end}]  ({span} heights)")
    print(f"file exists : {rep['exists']}")
    print(f"present     : {rep['present']}")
    print(f"missing     : {rep['missing_count']}")
    print(f"duplicates  : {rep['duplicates']}")
    print(f"malformed   : {rep['malformed_lines']} (lines) + "
          f"{rep['bad_roots']} bad-root + {rep['missing_roots']} no-root")
    print(f"out-of-range: {rep['out_of_range']}")
    print(f"failure file: {failed_path} -> {failures} recorded failures")

    if rep["missing_examples"]:
        print(f"  first missing heights: {rep['missing_examples']}")
    if rep["dup_examples"]:
        print(f"  first duplicate heights: {rep['dup_examples']}")
    if rep["bad_examples"]:
        print(f"  first malformed entries: {rep['bad_examples']}")
    if rep["oor_examples"]:
        print(f"  first out-of-range heights: {rep['oor_examples']}")

    problems = []
    if not rep["exists"]:
        problems.append("output file does not exist")
    if rep["missing_count"] != 0:
        problems.append(f"{rep['missing_count']} heights missing")
    if rep["duplicates"] != 0:
        problems.append(f"{rep['duplicates']} duplicate heights")
    if rep["malformed_lines"] or rep["bad_roots"] or rep["missing_roots"]:
        problems.append("malformed records present")
    if rep["out_of_range"] != 0:
        problems.append(f"{rep['out_of_range']} out-of-range heights")
    if rep["present"] != span:
        problems.append(f"present {rep['present']} != expected {span}")
    if failures != 0:
        problems.append(f"{failures} heights recorded as failed")

    if problems:
        print("\nGATE: FAIL")
        for p in problems:
            print(f"  - {p}")
        return False
    print("\nGATE: PASS - exactly one well-formed root for every height in range")
    return True


# --------------------------------------------------------------------------- #
# Thread-safe iterator
# --------------------------------------------------------------------------- #
class AtomicMissing:
    """Thread-safe ``next()`` over a generator of missing heights."""

    def __init__(self, start: int, end: int, bits: bytearray) -> None:
        self._lock = threading.Lock()
        self._start = start
        self._end = end
        self._bits = bits
        self._next = start

    def next(self) -> int | None:
        with self._lock:
            h = self._next
            while h <= self._end:
                i = h - self._start
                h += 1
                if not (self._bits[i >> 3] & (1 << (i & 7))):
                    self._next = h
                    return i + self._start
            self._next = h
            return None

    @property
    def remaining(self) -> int:
        with self._lock:
            h = self._next
        n = 0
        while h <= self._end:
            i = h - self._start
            if not (self._bits[i >> 3] & (1 << (i & 7))):
                n += 1
            h += 1
        return n


# --------------------------------------------------------------------------- #
# Worker
# --------------------------------------------------------------------------- #
def worker(
    host: str,
    port: int,
    timeout: float,
    retries: int,
    backoff_base: float,
    backoff_max: float,
    source: AtomicMissing,
    counters: Counters,
    appender: JsonlAppender,
    fail_appender: JsonlAppender,
    stop_event: threading.Event,
) -> None:
    client = SeedClient(host, port, timeout)
    try:
        while not stop_event.is_set():
            height = source.next()
            if height is None:
                break
            root = None
            last_err = None
            for attempt in range(retries):
                try:
                    root = client.get_stateroot(height)
                    break
                except Exception as exc:  # noqa: BLE001 - fail closed
                    last_err = exc
                    client.close()  # drop possibly-poisoned keep-alive conn
                    if attempt < retries - 1 and not stop_event.is_set():
                        delay = min(backoff_max, backoff_base * (attempt + 1))
                        time.sleep(delay)
            if root is not None:
                appender.write(height, root)
                counters.inc_ok()
            else:
                # NEVER silently drop: record the failure so the gate fails.
                fail_appender.write_raw(
                    {"height": height, "error": f"{type(last_err).__name__}: {last_err}"}
                )
                counters.inc_failed()
                print(f"  [FAIL] height {height}: {last_err}")
    finally:
        client.close()


def progress_loop(
    counters: Counters,
    already: int,
    to_do: int,
    interval: float,
    stop_event: threading.Event,
    t_start: float,
) -> None:
    while not stop_event.wait(interval):
        ok, failed = counters.snapshot()
        done = ok + failed
        elapsed = max(0.001, time.perf_counter() - t_start)
        hps = done / elapsed
        remaining = max(0, to_do - done)
        eta = remaining / hps if hps > 0 else float("inf")
        print(
            f"[progress] done={done}/{to_do} (+{already} pre-existing) "
            f"ok={ok} failed={failed} hps={hps:.1f} "
            f"ETA={_fmt_eta(eta)} elapsed={_fmt_eta(elapsed)}",
            flush=True,
        )


def _fmt_eta(seconds: float) -> str:
    if seconds == float("inf"):
        return "inf"
    seconds = int(seconds)
    h, rem = divmod(seconds, 3600)
    m, s = divmod(rem, 60)
    if h:
        return f"{h}h{m:02d}m"
    if m:
        return f"{m}m{s:02d}s"
    return f"{s}s"


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def parse_seeds(values: list[str], port: int) -> list[tuple[str, int]]:
    seeds: list[tuple[str, int]] = []
    for raw in values:
        for item in raw.split(","):
            item = item.strip()
            if not item:
                continue
            if item.startswith("http://") or item.startswith("https://"):
                item = item.split("://", 1)[1]
            host, _, p = item.partition(":")
            host = host.strip("/")
            if not host:
                continue
            seeds.append((host, int(p) if p else port))
    # de-duplicate while preserving order
    seen = set()
    uniq: list[tuple[str, int]] = []
    for s in seeds:
        if s not in seen:
            seen.add(s)
            uniq.append(s)
    return uniq


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        description="Download + gate the C# reference state roots for a height range.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument("--start", type=int, default=DEFAULT_START, help="first height (inclusive)")
    p.add_argument("--end", type=int, default=DEFAULT_END, help="last height (inclusive)")
    p.add_argument("--output", default=DEFAULT_OUTPUT, help="JSONL output path")
    p.add_argument("--failed-output", default=None,
                   help="sidecar failure path (default: <output>.failed.jsonl)")
    p.add_argument("--workers-per-seed", type=int, default=DEFAULT_WORKERS_PER_SEED,
                   help="worker threads per endpoint")
    p.add_argument("--seeds", action="append", default=None,
                   help="endpoint(s); comma list or repeated (default: seed1..seed5.neo.org)")
    p.add_argument("--port", type=int, default=DEFAULT_PORT, help="default RPC port")
    p.add_argument("--timeout", type=float, default=30.0, help="per-request timeout (s)")
    p.add_argument("--retries", type=int, default=6, help="attempts per height")
    p.add_argument("--backoff-base", type=float, default=0.4, help="backoff base seconds")
    p.add_argument("--backoff-max", type=float, default=3.0, help="backoff cap seconds")
    p.add_argument("--progress-interval", type=float, default=30.0, help="progress log period (s)")
    p.add_argument("--fsync-every", type=int, default=2000, help="fsync every N records")
    p.add_argument("--verify-only", action="store_true",
                   help="do not fetch; only run the completeness gate")
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    if args.end < args.start:
        print(f"ERROR: --end ({args.end}) < --start ({args.start})", file=sys.stderr)
        return 2

    failed_path = args.failed_output or (args.output + ".failed.jsonl")

    if args.verify_only:
        ok = run_gate(args.output, args.start, args.end, failed_path)
        return 0 if ok else 1

    seeds = parse_seeds(args.seeds if args.seeds else DEFAULT_SEEDS, args.port)
    if not seeds:
        print("ERROR: no seeds configured", file=sys.stderr)
        return 2

    span = args.end - args.start + 1
    print(f"Range      : [{args.start}, {args.end}] ({span} heights)")
    print(f"Output     : {args.output}")
    print(f"Failures   : {failed_path}")
    print(f"Seeds      : {[f'{h}:{p}' for h, p in seeds]}")
    print(f"Workers    : {args.workers_per_seed}/seed x {len(seeds)} = "
          f"{args.workers_per_seed * len(seeds)} total")

    # Resume: scan present heights.
    bits, present, dups, malformed, oor = load_present_bitset(args.output, args.start, args.end)
    print(f"Pre-existing: {present} heights already present "
          f"(duplicates={dups}, malformed={malformed}, out-of-range={oor})")
    if dups:
        print(f"WARNING: {dups} duplicate lines already in {args.output} "
              f"-> the completeness gate will fail. Remove duplicates first.")

    to_do = span - present
    if to_do == 0:
        print("Nothing to fetch (range already complete). Running gate.")
        ok = run_gate(args.output, args.start, args.end, failed_path)
        return 0 if ok else 1

    appender = JsonlAppender(args.output, fsync_every=args.fsync_every)
    fail_appender = JsonlAppender(failed_path, fsync_every=args.fsync_every)
    counters = Counters()
    stop_event = threading.Event()
    source = AtomicMissing(args.start, args.end, bits)

    t_start = time.perf_counter()
    prog = threading.Thread(
        target=progress_loop,
        args=(counters, present, to_do, args.progress_interval, stop_event, t_start),
        daemon=True,
    )
    prog.start()

    workers: list[threading.Thread] = []
    for host, port in seeds:
        for _ in range(args.workers_per_seed):
            t = threading.Thread(
                target=worker,
                args=(host, port, args.timeout, args.retries, args.backoff_base,
                      args.backoff_max, source, counters, appender, fail_appender,
                      stop_event),
                daemon=True,
            )
            t.start()
            workers.append(t)

    try:
        for t in workers:
            t.join()
    except KeyboardInterrupt:
        print("\nInterrupted; signalling workers to stop...", file=sys.stderr)
        stop_event.set()
        for t in workers:
            t.join(timeout=5)

    stop_event.set()
    appender.close()
    fail_appender.close()

    ok_ct, fail_ct = counters.snapshot()
    elapsed = time.perf_counter() - t_start
    hps = ok_ct / elapsed if elapsed > 0 else 0.0
    print(f"\nFetch done: ok={ok_ct} failed={fail_ct} in {_fmt_eta(elapsed)} "
          f"({hps:.1f} heights/s), pre-existing={present}")

    gate_ok = run_gate(args.output, args.start, args.end, failed_path)
    return 0 if gate_ok else 1


if __name__ == "__main__":
    sys.exit(main())
