"""Robust C# reference probe: verify GAS account storage format + first-appearance.

Sanity-checks the getstate contract-relative key convention against a known
GAS holder, then re-checks the 21373 sender across several seeds.

Read-only.
"""
import base64
import http.client
import json

SEEDS = [
    ("seed1.neo.org", 10332),
    ("seed2.neo.org", 10332),
    ("seed3.neo.org", 10332),
]

GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"
NEO = "0xf563e0bd82f37e8970c36f5fea2d0a5f96d7c6f9"

# 21373 GAS-failure sender
SENDER = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"
# A genesis validator (guaranteed to hold GAS early) as a control
GEN_VALIDATORS = [
    "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fbcf1b0",
]


def rpc(seed, method, params):
    conn = http.client.HTTPConnection(seed[0], seed[1], timeout=30)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def root_at(seed, h):
    r = rpc(seed, "getstateroot", [h]).get("result", {})
    return r.get("roothash")


def acct_key_b64(script_hash_hex):
    return base64.b64encode(bytes([0x14]) + bytes.fromhex(script_hash_hex)).decode()


def main():
    seed = SEEDS[0]

    # Decode the CommitteeInfo value stored at root 5107.
    ci = "0xb776afb6ad0c11565e70f8ee1dd898da43e51be1"
    cand = "96949ed482e7c60aaeec691550f1b3d599146194"
    root5107 = root_at(seed, 5107)
    kb = base64.b64encode(bytes([0x77]) + bytes.fromhex(cand)).decode()
    st = rpc(seed, "getstate", [root5107, ci, kb]).get("result")
    print("CommitteeInfo @5107 raw:", st)
    if st:
        raw = base64.b64decode(st)
        print("CommitteeInfo @5107 decoded hex:", raw.hex())

    # Control: does getstate return GAS for the block-5107 sender at root 5107?
    print("\n=== control: GAS getstate for known accounts ===")
    for label, sh in [
        ("block5107 sender", "94611499d5b3f1501569ecae0ac6e782d49e9496"),
        ("21373 sender", SENDER),
    ]:
        g = rpc(seed, "getstate", [root5107, GAS, acct_key_b64(sh)]).get("result")
        print(f"  {label:20s} @5107 -> {g}")

    # Now the 21373 sender across heights & seeds
    print("\n=== 21373 sender GAS across heights/seeds ===")
    for h in (10000, 15000, 20000, 21287, 21288, 21372, 21373):
        row = [str(h)]
        for s in SEEDS:
            rt = root_at(s, h)
            g = rpc(s, "getstate", [rt, GAS, acct_key_b64(SENDER)]).get("result")
            row.append(f"{s[0]}={'Y' if g else 'n'}")
        print("  ", " ".join(row))


if __name__ == "__main__":
    main()
