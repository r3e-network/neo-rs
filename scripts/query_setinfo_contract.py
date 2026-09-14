"""Dump the CommitteeInfoContract NEF script + manifest in full."""
import base64
import http.client
import json

HOST, PORT = "seed1.neo.org", 10332
HASH = "0xb776afb6ad0c11565e70f8ee1dd898da43e51be1"


def rpc(method, params):
    c = http.client.HTTPConnection(HOST, PORT, timeout=30)
    c.request("POST", "/", json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}),
              {"Content-Type": "application/json"})
    raw = c.getresponse().read().decode()
    c.close()
    return json.loads(raw)


def main():
    cs = rpc("getcontractstate", [HASH])["result"]
    raw = base64.b64decode(cs["nef"]["script"])
    print("SCRIPT_LEN:", len(raw))
    print("SCRIPT_HEX:", raw.hex())
    m = cs["manifest"]
    print("NAME:", m["name"])
    for meth in m["abi"]["methods"]:
        print(f"  METHOD {meth['name']} offset={meth['offset']} params={[p['name']+':'+p['type'] for p in meth['parameters']]} ret={meth['returntype']}")
    print("EVENTS:", [(e["name"], e["parameters"]) for e in m["abi"].get("events", [])])


if __name__ == "__main__":
    main()
