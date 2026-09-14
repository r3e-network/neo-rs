"""Empirically determine GAS storage-key convention on the C# reference.

Uses findstates to enumerate GAS (and CommitteeInfo) storage entries at a
known root, so we can confirm the account prefix and re-derive the 21373
sender's first GAS-appearance height correctly.

Read-only.
"""
import base64
import http.client
import json

SEED = ("seed1.neo.org", 10332)

GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"
COMMITTEE_INFO = "0xb776afb6ad0c11565e70f8ee1dd898da43e51be1"


def rpc(method, params):
    conn = http.client.HTTPConnection(SEED[0], SEED[1], timeout=30)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def root_at(h):
    return rpc("getstateroot", [h]).get("result", {}).get("roothash")


def main():
    root = root_at(5107)
    print("root 5107:", root)

    for label, contract, prefix_hex in [
        ("GAS prefix 0x14", GAS, "14"),
        ("GAS prefix 0x0c", GAS, "0c"),
        ("GAS empty prefix", GAS, ""),
        ("CommitteeInfo 0x77", COMMITTEE_INFO, "77"),
    ]:
        pfx = base64.b64encode(bytes.fromhex(prefix_hex)).decode()
        res = rpc("findstates", [root, contract, pfx])
        if "error" in res:
            print(f"{label}: ERROR {res['error']}")
            continue
        result = res.get("result", {})
        results = result.get("results", [])
        print(f"{label}: {len(results)} entries, truncated={result.get('truncated')}")
        for ent in results[:2]:
            k = ent.get("key")
            v = ent.get("value")
            kb = base64.b64decode(k).hex() if k else None
            print(f"    key={kb} value={v}")


if __name__ == "__main__":
    main()
