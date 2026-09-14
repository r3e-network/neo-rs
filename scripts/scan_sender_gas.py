"""Map presence of the 21373 sender's GAS balance (coarse, bounded).

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
        return None
    return rpc("getstate", [rt, GAS, KEY]).get("result")


def main():
    stride = 200
    hits = []
    n = 0
    for h in range(5107, 21373, stride):
        n += 1
        p = present(h) is not None
        if p:
            hits.append(h)
        if n % 20 == 0:
            print(f"  ...scanned to h={h}, hits so far={len(hits)}", file=sys.stderr, flush=True)
    print(f"stride={stride} present-hits (n={len(hits)}):")
    print(hits)


if __name__ == "__main__":
    main()
