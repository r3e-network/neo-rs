"""Corrected search: first height at which the 21373 sender holds GAS.

Storage keys use the account's internal (little-endian) byte order = reversed
canonical hex. search uses key 0x14 + reverse(account).

Read-only.
"""
import base64
import http.client
import json

SEED = ("seed1.neo.org", 10332)
GAS = "0xd2a4cff31913016155e38e474a2c06d08be276cf"

SENDER_21373 = "ed369077652ddd55bd7696df93fe49c0bb40d3bc"
SENDER_5107 = "94611499d5b3f1501569ecae0ac6e782d49e9496"


def rpc(method, params):
    conn = http.client.HTTPConnection(SEED[0], SEED[1], timeout=40)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


def root_at(h):
    return rpc("getstateroot", [h]).get("result", {}).get("roothash")


def key_for(account_hex):
    raw = bytes.fromhex(account_hex)[::-1]
    return base64.b64encode(bytes([0x14]) + raw).decode()


def balance(account_hex, h):
    return rpc("getstate", [root_at(h), GAS, key_for(account_hex)]).get("result")


def main():
    print("=== sanity: 21373 sender present at 21372, absent at 21373 ===")
    print("  @21372:", balance(SENDER_21373, 21372))
    print("  @21373:", balance(SENDER_21373, 21373))

    print("\n=== block-5107 sender at 5107 (its own setInfo block) ===")
    print("  @5106:", balance(SENDER_5107, 5106))
    print("  @5107:", balance(SENDER_5107, 5107))

    print("\n=== first appearance search for 21373 sender (stride scan) ===")
    lo = 5107
    first_present = None
    h = 5107
    while h <= 21373:
        b = balance(SENDER_21373, h)
        if b is not None:
            first_present = h
            break
        h += 500
    print("  coarse first present (step 500):", first_present)

    if first_present:
        start = max(5107, first_present - 500)
        ref = None
        for h in range(start, first_present + 1):
            if balance(SENDER_21373, h) is not None:
                ref = h
                break
        print("  refined first present (step 1):", ref)
        if ref:
            print("  balance just before:", balance(SENDER_21373, ref - 1), "at", ref - 1)
            print("  balance at first:", balance(SENDER_21373, ref))


if __name__ == "__main__":
    main()
