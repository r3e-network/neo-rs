#!/usr/bin/env python3
"""Auto-restart watchdog for neo-node state root sync."""
import subprocess, time, http.client, json, gzip

NODE = "/home/neo/git/neo-rs/target/release/neo-node"
CONFIG = "/home/neo/git/neo-rs/config/mainnet-stateroot.toml"
LOG = "/home/neo/git/neo-rs/logs/neo-node-stateroot.log"

def get_height():
    try:
        c = http.client.HTTPConnection("127.0.0.1", 20332, timeout=5)
        c.request("POST", "/", json.dumps({"jsonrpc":"2.0","method":"getstateheight","params":[],"id":1}), {"Content-Type":"application/json"})
        r = c.getresponse().read()
        if r[:2] == b"\x1f\x8b": r = gzip.decompress(r)
        c.close()
        return json.loads(r)["result"]["localrootindex"]
    except:
        return -1

def start():
    f = open(LOG, "a")
    return subprocess.Popen([NODE, "--config", CONFIG, "--state-root", "--state-root-full-state"], stdout=f, stderr=f)

proc = start()
last_h, stall, restarts, start_h = -1, 0, 1, -1
t0 = time.time()
print(f"[{time.strftime('%H:%M:%S')}] watchdog started", flush=True)

while True:
    time.sleep(5)
    if proc.poll() is not None:
        proc = start(); restarts += 1; last_h = -1; stall = 0
        time.sleep(8); continue
    h = get_height()
    if h < 0: continue
    if start_h < 0: start_h = h
    if h == last_h:
        stall += 5
        if stall >= 20:
            proc.kill(); proc.wait()
            elapsed = time.time() - t0
            total = h - start_h
            rate = total / elapsed if elapsed > 0 else 0
            remaining = 9092000 - h
            eta_days = remaining / rate / 86400 if rate > 0 else 999
            print(f"[{time.strftime('%H:%M:%S')}] h={h:,} R={restarts} total=+{total:,} rate={rate:.1f}/s ETA={eta_days:.1f}d", flush=True)
            proc = start(); restarts += 1
            stall = 0; last_h = -1; time.sleep(8)
    else:
        stall = 0
    last_h = h
