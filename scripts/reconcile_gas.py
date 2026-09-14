"""Reconcile getstate vs findstates for GAS, and locate the 21373 sender.

Read-only.
"""
import base64
import http.client
import json

SEED = ("seed1.neo.org", 10332)
GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"


def rpc(method, params):
    conn = http.client.HTTPConnection(SEED[0], SEED[1], timeout=30)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def root_at(h):
    return rpc("getstateroot", [h]).get("result", {}).get("roothash")


def b64key(hexstr):
    return base64.b64encode(bytes.fromhex(hexstr)).decode()


def main():
    root = root_at(5107)

    # 1. getstate for a KNOWN key that findstates reported.
    known_key = "1413ce9b35465f129f59af8836e2a3f902f28b1cbc"
    # findstates returns contract-relative key; try as-is and with id stripped
    for label, key in [
        ("as returned (0x14+acct)", known_key),
        ("without 0x14", known_key[2:]),
    ]:
        g = rpc("getstate", [root, GAS, b64key(key)]).get("result")
        print(f"getstate GAS {label:24s} -> {g}")

    # 2. Is the block-5107 sender among the 31 account keys?
    sender_5107 = "94611499d5b3f1501569ecae0ac6e782d49e9496"
    sender_21373 = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"
    res = rpc("findstates", [root, GAS, b64key("14")]).get("result", {})
    keys = [base64.b64decode(e["key"]).hex() for e in res.get("results", [])]
    print(f"\n# GAS account keys @5107: {len(keys)}")
    print("  5107 sender present:", ("14" + sender_5107) in keys)
    print("  21373 sender present:", ("14" + sender_21373) in keys)

    # 3. 21373 sender across heights via findstates presence + getstate
    print("\n# 21373 sender GAS presence across heights")
    for h in (5107, 15000, 20000, 21000, 21287, 21288, 21372, 21373):
        rt = root_at(h)
        ress = rpc("findstates", [rt, GAS, b64key("14" + sender_21373)]).get("result", {})
        present = len(ress.get("results", [])) > 0
        g = rpc("getstate", [rt, GAS, b64key("14" + sender_21373)]).get("result")
        print(f"  h={h:6d} findstates_present={present} getstate={g}")


if __name__ == "__main__":
    main()
