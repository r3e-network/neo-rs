#!/usr/bin/env python3
"""Keep-alive state-root fetcher (bisect helper).

The stock downloaders open a fresh TCP connection per RPC call, which on Windows
exhausts the ephemeral port range (WinError 10048) at high concurrency. This
helper pins one persistent HTTP connection per worker thread, so it sustains a
high request rate without a TIME_WAIT storm.

Resumable: reads existing heights from --output and only fetches the missing
ones in [0, --target). Appends results, then rewrites the file sorted/deduped.
"""
import argparse
import http.client
import json
import os
import socket
import threading
import time
import urllib.parse

SEEDS = [f"http://seed{i}.neo.org:10332" for i in (1, 2, 3, 4, 5)]


def load_existing(path):
    have = set()
    if os.path.exists(path):
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    have.add(json.loads(line)["height"])
                except Exception:
                    pass
    return have


class Worker(threading.Thread):
    def __init__(self, url, queue, results, lock, stop):
        super().__init__(daemon=True)
        self.url = url
        self.queue = queue
        self.results = results
        self.lock = lock
        self.stop = stop
        u = urllib.parse.urlparse(url)
        self.host = u.hostname
        self.port = u.port or 80
        self.conn = None
        self.fetched = 0

    def _connect(self):
        if self.conn is not None:
            try:
                self.conn.close()
            except Exception:
                pass
        self.conn = http.client.HTTPConnection(self.host, self.port, timeout=20)

    def _request(self, height):
        body = json.dumps(
            {"jsonrpc": "2.0", "id": 1, "method": "getstateroot", "params": [height]}
        )
        self.conn.request(
            "POST", "/", body=body, headers={"Content-Type": "application/json"}
        )
        resp = self.conn.getresponse()
        raw = resp.read()
        return json.loads(raw.decode())

    def run(self):
        self._connect()
        while not self.stop.is_set():
            try:
                height = self.queue.get_nowait()
            except Exception:
                break
            ok = False
            for attempt in range(6):
                try:
                    obj = self._request(height)
                    root = obj["result"]["roothash"]
                    with self.lock:
                        self.results.append((height, root))
                    self.fetched += 1
                    ok = True
                    break
                except Exception:
                    self._connect()
                    time.sleep(0.5 * (attempt + 1))
            if not ok:
                # requeue once; if permanently failing it will be retried next run
                with self.lock:
                    self.queue.put(height)
                time.sleep(0.2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", type=int, default=30000)
    ap.add_argument("--output", default="data/reference_stateroots.jsonl")
    ap.add_argument("--threads-per-seed", type=int, default=4)
    args = ap.parse_args()

    existing = load_existing(args.output)
    print(f"existing heights: {len(existing)}", flush=True)
    missing = [h for h in range(0, args.target) if h not in existing]
    if not missing:
        print("nothing to fetch", flush=True)
        return
    print(f"fetching {len(missing)} missing heights", flush=True)

    import queue as _q

    q = _q.Queue()
    for h in missing:
        q.put(h)
    results = []
    lock = threading.Lock()
    stop = threading.Event()

    workers = []
    for url in SEEDS:
        for _ in range(args.threads_per_seed):
            w = Worker(url, q, results, lock, stop)
            w.start()
            workers.append(w)

    start = time.time()
    written = 0
    while any(w.is_alive() for w in workers):
        time.sleep(15)
        with lock:
            done = len(results)
            new = results[written:]
            written = done
        if new:
            with open(args.output, "a") as f:
                for h, root in new:
                    f.write(json.dumps({"height": h, "roothash": root}) + "\n")
                f.flush()
        print(
            f"  +{done}/{len(missing)} in {int(time.time()-start)}s "
            f"({done/max(1,time.time()-start):.1f}/s)",
            flush=True,
        )

    # merge into canonical
    by = {}
    if os.path.exists(args.output):
        for line in open(args.output):
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
                by[r["height"]] = r["roothash"]
            except Exception:
                pass
    for h, root in results:
        by[h] = root
    tmp = args.output + ".tmp"
    with open(tmp, "w") as f:
        for h in sorted(by):
            f.write(json.dumps({"height": h, "roothash": by[h]}) + "\n")
    os.replace(tmp, args.output)
    print(f"wrote {len(by)} heights to {args.output}", flush=True)


if __name__ == "__main__":
    main()
