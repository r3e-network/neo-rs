"""Refine first GAS-appearance height of the 21373 sender.

Read-only.
"""
import base64
import http.client
import json
import sys

HOST, PORT = "seed1.neo.org", 10332
GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"
SENDER = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"
KEY = base64.b64encode(bytes([0x14]) + bytes.fromhex(SENDER)[::-1]).decode()

_conn = None


def rpc(method, params):
    global _conn
    if _conn is None:
        _conn = http.client.HTTPConnection(HOST, PORT, timeout=30)
    try:
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
        _conn.request("POST", "/", body, {"Content-Type": "application/json"})
        return json.loads(_conn.getresponse().read().decode())
    except Exception:
        try:
            _conn.close()
        except Exception:
            pass
        _conn = None
        return {}


def present(h):
    rt = rpc("getstateroot", [h]).get("result", {}).get("roothash")
    if not rt:
        return False
    return rpc("getstate", [rt, GAS, KEY]).get("result") is not None


def main():
    first = None
    for h in range(21100, 21373):
        if present(h):
            first = h
            break
        if h % 50 == 0:
            print(f"  ...to {h}", file=sys.stderr, flush=True)
    print("first present height:", first)
    if first:
        for h in (first - 1, first, first + 1):
            print(f"   h={h} present={present(h)}")


if __name__ == "__main__":
    main()
