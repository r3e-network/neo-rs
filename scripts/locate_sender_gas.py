"""Enumerate GAS holders across heights to locate the 21373 sender.

Read-only.
"""
import base64
import http.client
import json

SEEDS = [("seed1.neo.org", 10332), ("seed2.neo.org", 10332)]
GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"
SENDER = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"
SENDER_TAIL = SENDER  # 20-byte account


def rpc(seed, method, params):
    conn = http.client.HTTPConnection(seed[0], seed[1], timeout=40)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def root_at(seed, h):
    return rpc(seed, "getstateroot", [h]).get("result", {}).get("roothash")


def gas_keys(seed, root):
    res = rpc(seed, "findstates", [root, GAS, base64.b64encode(b"").decode(), ""]).get("result", {})
    out = []
    for e in res.get("results", []):
        out.append(base64.b64decode(e["key"]).hex())
    return out


def main():
    for h in (21287, 21288, 21289, 21290, 21300, 21350, 21371, 21372):
        row = [f"h={h}"]
        for s in SEEDS:
            rt = root_at(s, h)
            keys = gas_keys(s, rt)
            hit = [k for k in keys if k.endswith(SENDER_TAIL)]
            row.append(f"{s[0]}: n={len(keys)} sender={'YES' if hit else 'no'}")
        print("  ".join(row))

    # Full key list at 21372 (seed1) to eyeball
    rt = root_at(SEEDS[0], 21372)
    keys = gas_keys(SEEDS[0], rt)
    print(f"\n=== all GAS keys @21372 ({len(keys)}) ===")
    for k in sorted(keys):
        print("   ", k)


if __name__ == "__main__":
    main()
