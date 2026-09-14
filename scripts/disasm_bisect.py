"""Correct Neo N3 disassembler for the two bisect scripts. Read-only."""
import base64
import http.client
import json

HOST, PORT = "seed1.neo.org", 10332

# Neo N3 opcodes we need (from neo-vm/src/vm/opcode.rs)
SIMPLE = {
    0x00: "PUSHINT8", 0x01: "PUSHINT16", 0x02: "PUSHINT32", 0x03: "PUSHINT64",
    0x04: "PUSHINT128", 0x05: "PUSHINT256", 0x08: "PUSHT", 0x09: "PUSHF",
    0x0A: "PUSHA", 0x0B: "PUSHNULL", 0x0F: "PUSHM1", 0x10: "PUSH0",
    0x37: "CALLT", 0x38: "ABORT", 0x39: "ASSERT", 0x3A: "THROW", 0x40: "RET",
    0x45: "DROP", 0x4A: "DUP", 0xCF: "APPEND", 0xBE: "PACKMAP", 0xBF: "PACKSTRUCT",
    0xC0: "PACK", 0xC1: "UNPACK", 0xC2: "NEWARRAY0", 0xC3: "NEWARRAY",
    0xC8: "NEWMAP", 0xCE: "PICKITEM", 0xD0: "SETITEM",
}
OPERAND = {0x00: 1, 0x01: 2, 0x02: 4, 0x03: 8, 0x04: 16, 0x05: 32, 0x0A: 4, 0x37: 2}


def ascii_preview(b):
    s = "".join(chr(c) if 32 <= c < 127 else "." for c in b)
    return s


def disasm(code):
    out, i = [], 0
    while i < len(code):
        op = code[i]
        if 0x11 <= op <= 0x20:
            out.append(f"{i:04x}: PUSH{op-0x10}"); i += 1
        elif op == 0x0C:
            n = code[i+1]; d = code[i+2:i+2+n]
            out.append(f"{i:04x}: PUSHDATA1({n}) 0x{d.hex()} \"{ascii_preview(d)}\""); i += 2+n
        elif op == 0x0D:
            n = int.from_bytes(code[i+1:i+3], "little"); d = code[i+3:i+3+n]
            out.append(f"{i:04x}: PUSHDATA2({n}) 0x{d.hex()}"); i += 3+n
        elif op == 0x41:
            out.append(f"{i:04x}: SYSCALL 0x{code[i+1:i+5].hex()}"); i += 5
        elif op in OPERAND:
            n = OPERAND[op]; d = code[i+1:i+1+n]
            out.append(f"{i:04x}: {SIMPLE[op]} 0x{d.hex()} ({int.from_bytes(d,'little',signed=True)})"); i += 1+n
        elif op in SIMPLE:
            out.append(f"{i:04x}: {SIMPLE[op]}"); i += 1
        else:
            out.append(f"{i:04x}: OP_{op:02x}"); i += 1
    return out


def get_block_tx(h):
    c = http.client.HTTPConnection(HOST, PORT, timeout=30)
    c.request("POST", "/", json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getblock", "params": [h, 1]}),
              {"Content-Type": "application/json"})
    b = json.loads(c.getresponse().read().decode())["result"]
    c.close()
    return b


def main():
    for h in (5107, 21373):
        b = get_block_tx(h)
        print(f"===== block {h} ({b['hash']}) =====")
        for tx in b["tx"]:
            print(f"  tx {tx['hash']}")
            print(f"    sender={tx['signers'][0]['account']} sysfee={tx['sysfee']} netfee={tx['netfee']}")
            code = base64.b64decode(tx["script"])
            size = len(code)
            for line in disasm(code):
                print("     ", line)
            print(f"    (script len {size} bytes)")
        print()


if __name__ == "__main__":
    main()
