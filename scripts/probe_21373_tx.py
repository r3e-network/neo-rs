"""Deep dive: block 21373 tx — full app-log notifications + script disasm.

Read-only.
"""
import base64
import http.client
import json

SEED = ("seed1.neo.org", 10332)
TX = "0xc68aac4b0bb9e88bd42086c50cebe648ad28726d2849ff73faeb93985e510587"


def rpc(method, params):
    conn = http.client.HTTPConnection(SEED[0], SEED[1], timeout=40)
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    conn.request("POST", "/", body, {"Content-Type": "application/json"})
    raw = conn.getresponse().read().decode()
    conn.close()
    return json.loads(raw)


OP = {
    0x00: "PUSHBYTES0", 0x0F: "PUSHM1", 0x10: "PUSH0",
    0x21: "PUSHINT8", 0x22: "PUSHINT16", 0x23: "PUSHINT32",
    0x28: "PUSHINT64", 0x29: "PUSHINT128", 0x2A: "PUSHINT256",
    0x2B: "PUSHA", 0x30: "PUSHNULL", 0x31: "PUSHDATA1",
    0x32: "PUSHDATA2", 0x33: "PUSHDATA4", 0x34: "PUSHM1?",
    0x40: "PUSH16", 0x41: "SYSCALL", 0x45: "CALL",
    0x51: "DROP", 0x6C: "DUP", 0x6E: "APPEND", 0x87: "NEWARRAY0",
    0xC0: "SWAP", 0xC1: "ROT", 0xC2: "CAT", 0xAE: "CHECKMULTISIG",
    0xAC: "CHECKSIG", 0x9A: "ASSERT",
}


def disasm(code):
    out, i = [], 0
    while i < len(code):
        op = code[i]
        if 0x11 <= op <= 0x20:
            out.append(f"{i:04x}: PUSH{op - 0x10}"); i += 1
        elif 0x01 <= op <= 0x4B:
            n = op
            data = code[i + 1:i + 1 + n]
            out.append(f"{i:04x}: PUSHDATA{n} 0x{data.hex()} ({_try_ascii(data)})")
            i += 1 + n
        elif op == 0x0C:
            n = code[i + 1]
            data = code[i + 2:i + 2 + n]
            out.append(f"{i:04x}: PUSHDATA1({n}) 0x{data.hex()} ({_try_ascii(data)})")
            i += 2 + n
        elif op == 0x0D:
            n = int.from_bytes(code[i + 1:i + 3], "little")
            data = code[i + 3:i + 3 + n]
            out.append(f"{i:04x}: PUSHDATA2({n}) 0x{data.hex()} ({_try_ascii(data)})")
            i += 3 + n
        elif op == 0x0E:
            n = int.from_bytes(code[i + 1:i + 5], "little")
            data = code[i + 5:i + 5 + n]
            out.append(f"{i:04x}: PUSHDATA4({n}) 0x{data.hex()} ({_try_ascii(data)})")
            i += 5 + n
        elif op == 0x41:
            h = code[i + 1:i + 5]
            out.append(f"{i:04x}: SYSCALL 0x{h.hex()}")
            i += 5
        else:
            out.append(f"{i:04x}: {OP.get(op, f'OP_{op:02x}')}")
            i += 1
    return out


def _try_ascii(b):
    try:
        s = b.decode("ascii")
        return f'"{s}"' if s.isprintable() else ""
    except Exception:
        return ""


def main():
    b = rpc("getblock", [21373, 1])["result"]
    tx = b["tx"][0]
    raw = base64.b64decode(tx["script"])
    print("=== tx script disassembly ===")
    for line in disasm(raw):
        print("  ", line)

    print("\n=== app log notifications ===")
    lg = rpc("getapplicationlog", [TX])["result"]
    ex = lg["executions"][0]
    print("vmstate:", ex["vmstate"], "gasconsumed:", ex["gasconsumed"])
    for n in ex.get("notifications", []):
        print(f"  notif contract={n['contract']} event={n['eventname']} state={json.dumps(n['state'])[:200]}")

    # identify the called contract
    for h in ["0xcf76e28bd0062c4a478ee35561011319f3cfa4d2"]:
        cs = rpc("getcontractstate", [h]).get("result")
        if cs:
            print(f"\ncontract {h}: manifest.name={cs.get('manifest',{}).get('name')} updatecounter={cs.get('updatecounter')}")


if __name__ == "__main__":
    main()
