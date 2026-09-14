"""One-off evidence re-verification for the state-root bisect investigation.

Re-queries the C# MainNet reference (seed1.neo.org:10332) for:
  1. The CommitteeInfoContract storage entry at root 5107 (the suspected
     Rust-dropped write).
  2. The first height at which the block-21373 GAS failure sender
     (0xed369077652ddd55bd7696df93fe49c0bb40d3bc) appears in GAS storage.

Read-only; does not touch any local store.
"""
import base64
import http.client
import json

SEED = ("seed1.neo.org", 10332)
ROOT_5107 = "0x941868fa4eb5cd80807dc17c0ef05e6cb1c6b57a9d2899ea274fa8b8c69604b1"

COMMITTEE_INFO = "0xb776afb6ad0c11565e70f8ee1dd898da43e51be1"
CANDIDATE = "96949ed482e7c60aaeec691550f1b3d599146194"

GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"
SENDER_21373 = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"


def rpc(host, port, method, params):
    conn = http.client.HTTPConnection(host, port, timeout=30)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def main():
    key_body = bytes([0x77]) + bytes.fromhex(CANDIDATE)
    key_b64 = base64.b64encode(key_body).decode()

    print("=== C# getstate CommitteeInfo key at root 5107 ===")
    res = rpc(*SEED, "getstate", [ROOT_5107, COMMITTEE_INFO, key_b64])
    print(json.dumps(res)[:600])

    print("\n=== C# findstates CommitteeInfo prefix 0x77 at root 5107 ===")
    res = rpc(
        *SEED,
        "findstates",
        [ROOT_5107, COMMITTEE_INFO, base64.b64encode(bytes([0x77])).decode(), 5],
    )
    print(json.dumps(res)[:1000])

    acct = bytes.fromhex(SENDER_21373)
    gas_key_b64 = base64.b64encode(bytes([0x14]) + acct).decode()

    print("\n=== GAS entry for 0xed369077 at selected heights ===")
    for h in (5107, 21287, 21288, 21372, 21373):
        rr = rpc(*SEED, "getstateroot", [h])
        root = rr.get("result", {}).get("roothash")
        g = rpc(*SEED, "getstate", [root, GAS, gas_key_b64])
        state = g.get("result")
        print(h, str(root)[:22], "...", json.dumps(state)[:140])


if __name__ == "__main__":
    main()
