"""Ground-truth pull for block 21373 (and 5107) from the C# reference.

Read-only.
"""
import base64
import http.client
import json

SEED = ("seed1.neo.org", 10332)
GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"

SENDER_21373 = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"


def rpc(method, params):
    conn = http.client.HTTPConnection(SEED[0], SEED[1], timeout=40)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def root_at(h):
    return rpc("getstateroot", [h]).get("result", {}).get("roothash")


def gas_entries(root):
    res = rpc("findstates", [root, GAS, base64.b64encode(b"").decode(), ""]).get("result", {})
    out = {}
    for e in res.get("results", []):
        kb = base64.b64decode(e["key"]).hex()
        out[kb] = e.get("value")
    return out


def main():
    b = rpc("getblock", [21373, 1]).get("result", {})
    print("=== block 21373 ===")
    print("hash:", b.get("hash"), "index:", b.get("index"), "txcount:", b.get("txcount"))
    for tx in b.get("tx", []):
        print("\n  tx", tx["hash"])
        print("    sysfee:", tx.get("sysfee"), "netfee:", tx.get("netfee"))
        sgn = tx.get("signers", [])
        print("    signers:", [(s.get("account"), s.get("scopes")) for s in sgn])
        # First signer == sender
    print("\n  all tx hashes:", [t["hash"] for t in b.get("tx", [])])

    # app logs
    print("\n=== application logs (vmstate) block 21373 ===")
    for tx in b.get("tx", []):
        h = tx["hash"]
        lg = rpc("getapplicationlog", [h]).get("result", {})
        ex = lg.get("executions", [{}])[0]
        print(f"  {h[:18]}.. vmstate={ex.get('vmstate')} gas={ex.get('gasconsumed')} exc={ex.get('exception')}")

    # GAS entries at 21373 and search for sender
    print("\n=== GAS storage at root 21373 ===")
    ent = gas_entries(root_at(21373))
    print("  total GAS entries:", len(ent))
    hit = [k for k in ent if k.endswith(SENDER_21373)]
    print("  keys containing sender:", hit)
    # also try as contract account (0x14 prefix)
    print("  exact key 14+", SENDER_21373, ":", ent.get("14" + SENDER_21373))


if __name__ == "__main__":
    main()
