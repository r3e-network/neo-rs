"""Inspect block 21288: what credits the 21373 sender with GAS.

Read-only.
"""
import base64
import http.client
import json

HOST, PORT = "seed1.neo.org", 10332
SENDER = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"


def rpc(method, params):
    conn = http.client.HTTPConnection(HOST, PORT, timeout=40)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def le_to_hex(b64):
    return base64.b64decode(b64)[::-1].hex()


def main():
    b = rpc("getblock", [21288, 1])["result"]
    print("block 21288 hash:", b["hash"], "txs:", len(b["tx"]))
    for tx in b["tx"]:
        print(f"\n  tx {tx['hash']}")
        print("    sysfee:", tx["sysfee"], "netfee:", tx["netfee"])
        print("    signers:", [(s["account"], s["scopes"]) for s in tx["signers"]])
        lg = rpc("getapplicationlog", [tx["hash"]])["result"]["executions"][0]
        print("    vmstate:", lg["vmstate"], "gas:", lg["gasconsumed"])
        for n in lg.get("notifications", []):
            if n["eventname"] == "Transfer":
                vals = n["state"]["value"]
                frm = le_to_hex(vals[0]["value"])
                to = le_to_hex(vals[1]["value"])
                amt = vals[2]["value"]
                mark = ""
                if to == SENDER or frm == SENDER:
                    mark = "  <== SENDER"
                print(f"      GAS Transfer from=0x{frm} to=0x{to} amount={amt}{mark}")
            else:
                print(f"      {n['eventname']} contract={n['contract']}")


if __name__ == "__main__":
    main()
