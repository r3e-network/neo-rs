#!/usr/bin/env python3
"""Generate syscall differential-execution vectors for the hosted ApplicationEngine.

These vectors require a *hosted* ApplicationEngine (syscalls + native-contract
initialization), unlike vectors/vm/diff-vectors.json which runs on a bare
engine. They are consumed by BOTH:

  - Rust  : neo-core/examples/vm_syscall_runner.rs
  - C#    : tools/csharp-app-runner (Neo 3.10.1)

Design rules
------------
1. Opcode bytes are read from `neo-vm/src/vm/opcode.rs` (source of truth), so
   a byte-value drift cannot silently desync the vectors from the VM.
2. Syscall hashes are the first 4 bytes of SHA-256(method), emitted little-
   endian, matching neo-rs `interop_hash` and the C# interop service. The
   SYSCALL operand is those 4 bytes.
3. The vector set is restricted to syscalls whose semantics are deterministic
   under a *fixed, pinned engine environment* that BOTH runners construct
   identically (see the runner source constants):
       - ProtocolSettings defaults: network = 0, address_version = 53
       - TriggerType = Application (64)
       - a minimal Transaction container (its exact hash is NOT load-bearing
         because we only exercise crypto FALSE / FAULT paths)
       - a persisting block whose header timestamp = 1700000000
   Storage / native-contract value transfers / cross-contract calls are NOT
   covered (see reports/formal/syscall-diff-*.md for the boundary).
4. Every script ends with RET; the single result item is unambiguous.

Usage:
  python scripts/gen-syscall-vectors.py
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OPCODE_RS = REPO / "neo-vm" / "src" / "vm" / "opcode.rs"
OUT = REPO / "vectors" / "vm" / "syscall-vectors.json"

# A valid secp256r1 (NIST P-256) compressed public key. Used only for the
# crypto FALSE / FAULT paths; a zero signature never verifies regardless of
# the message, so this is deterministic on both implementations.
VALID_PUBKEY = bytes.fromhex("024b817ef37f2fc3d4a33fe36687e592d9f30fe24b3e28187dc8f12b3b3b2b839e")
ZERO_SIG = bytes(64)


def load_opcodes() -> dict[str, int]:
    src = OPCODE_RS.read_text(encoding="utf-8")
    table: dict[str, int] = {}
    for m in re.finditer(r"^\s*([A-Z][A-Z0-9_]*)\s*=\s*(0x[0-9A-Fa-f]+)\s*,", src, re.M):
        table[m.group(1)] = int(m.group(2), 16)
    if not table:
        raise SystemExit(f"no opcodes parsed from {OPCODE_RS}")
    return table


def syscall_bytes(name: str) -> list[int]:
    """First 4 bytes of SHA-256(name) as the SYSCALL operand (little-endian)."""
    digest = hashlib.sha256(name.encode("ascii")).digest()
    return list(digest[:4])


class Builder:
    def __init__(self, ops: dict[str, int]):
        self.ops = ops
        self.buf: list[int] = []

    def op(self, name: str) -> "Builder":
        self.buf.append(self.ops[name])
        return self

    def push_int(self, value: int) -> "Builder":
        if value == -1:
            self.buf.append(self.ops["PUSHM1"])
        elif value == 0:
            self.buf.append(self.ops["PUSH0"])
        elif 1 <= value <= 16:
            self.buf.append(self.ops[f"PUSH{value}"])
        elif -128 <= value <= 127:
            self.buf += [self.ops["PUSHINT8"], value & 0xFF]
        elif -32768 <= value <= 32767:
            self.buf.append(self.ops["PUSHINT16"])
            self.buf += list((value & 0xFFFF).to_bytes(2, "little"))
        elif -(2**31) <= value <= 2**31 - 1:
            self.buf.append(self.ops["PUSHINT32"])
            self.buf += list((value & 0xFFFFFFFF).to_bytes(4, "little"))
        else:
            self.buf.append(self.ops["PUSHINT64"])
            self.buf += list((value & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little"))
        return self

    def push_bytes(self, data: bytes) -> "Builder":
        n = len(data)
        if n < 0x100:
            self.buf += [self.ops["PUSHDATA1"], n]
        elif n < 0x10000:
            self.buf += [self.ops["PUSHDATA2"]]
            self.buf += list(n.to_bytes(2, "little"))
        else:
            self.buf += [self.ops["PUSHDATA4"]]
            self.buf += list(n.to_bytes(4, "little"))
        self.buf += list(data)
        return self

    def pack_array(self, items: list[bytes]) -> "Builder":
        for it in items:
            self.push_bytes(it)
        self.push_int(len(items))
        self.op("PACK")
        return self

    def syscall(self, name: str) -> "Builder":
        self.buf.append(self.ops["SYSCALL"])
        self.buf += syscall_bytes(name)
        return self

    def hex(self) -> str:
        return "".join(f"{b:02x}" for b in self.buf)


def build_vectors(ops: dict[str, int]) -> list[dict]:
    vectors: list[dict] = []

    def add(name: str, group: str, b: Builder, note: str = "") -> None:
        vectors.append(
            {"name": name, "group": group, "note": note, "script": b.hex()}
        )

    # ---- Runtime metadata (pure, no storage) ----------------------------
    add("rs.platform", "runtime",
        Builder(ops).syscall("System.Runtime.Platform").op("RET"),
        "platform -> 'NEO' ByteString")
    add("rs.network", "runtime",
        Builder(ops).syscall("System.Runtime.GetNetwork").op("RET"),
        "network -> Integer (both impls default network=0)")
    add("rs.address_version", "runtime",
        Builder(ops).syscall("System.Runtime.GetAddressVersion").op("RET"),
        "address_version -> Integer 53 (both impls default)")
    add("rs.trigger", "runtime",
        Builder(ops).syscall("System.Runtime.GetTrigger").op("RET"),
        "trigger -> Integer 64 (Application)")
    add("rs.invocation_counter", "runtime",
        Builder(ops).syscall("System.Runtime.GetInvocationCounter").op("RET"),
        "invocation counter -> 1 for entry script")
    add("rs.time", "runtime",
        Builder(ops).syscall("System.Runtime.GetTime").op("RET"),
        "persisting-block timestamp -> Integer 1700000000")
    add("rs.calling_hash", "runtime",
        Builder(ops).syscall("System.Runtime.GetCallingScriptHash").op("RET"),
        "calling script hash -> Null at entry context")
    add("rs.executing_hash", "runtime",
        Builder(ops).syscall("System.Runtime.GetExecutingScriptHash").op("RET"),
        "executing script hash -> script hash of loaded bytes")
    add("rs.entry_hash", "runtime",
        Builder(ops).syscall("System.Runtime.GetEntryScriptHash").op("RET"),
        "entry script hash -> script hash of loaded bytes")

    # ---- Crypto ---------------------------------------------------------
    add("crypto.checksig.false", "crypto",
        Builder(ops).push_bytes(ZERO_SIG).push_bytes(VALID_PUBKEY)
        .syscall("System.Crypto.CheckSig").op("RET"),
        "valid pubkey + zero signature -> false (message-independent)")
    add("crypto.checksig.invalid_pubkey_len", "crypto",
        Builder(ops).push_bytes(ZERO_SIG).push_bytes(bytes(70))
        .syscall("System.Crypto.CheckSig").op("RET"),
        "70-byte pubkey -> FAULT (invalid public key length)")
    add("crypto.checkmultisig.empty_pubkeys", "crypto",
        Builder(ops).pack_array([ZERO_SIG]).op("NEWARRAY0")
        .syscall("System.Crypto.CheckMultisig").op("RET"),
        "empty pubkey array -> FAULT (invalid public key count)")
    add("crypto.checkmultisig.invalid_sig", "crypto",
        Builder(ops).pack_array([ZERO_SIG, ZERO_SIG])
        .pack_array([VALID_PUBKEY, VALID_PUBKEY])
        .syscall("System.Crypto.CheckMultisig").op("RET"),
        "2-of-2 with all-zero signatures -> false")

    # ---- CheckWitness (benign, no matching signer) ----------------------
    add("cw.not_signer", "witness",
        Builder(ops).push_bytes(bytes(20))
        .syscall("System.Runtime.CheckWitness").op("RET"),
        "non-signer 20-byte hash -> false")

    return vectors


def main() -> int:
    ops = load_opcodes()
    vectors = build_vectors(ops)
    seen: set[str] = set()
    for v in vectors:
        if v["name"] in seen:
            raise SystemExit(f"duplicate vector name: {v['name']}")
        seen.add(v["name"])

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        json.dumps(
            {
                "description": (
                    "Syscall differential-execution vectors (hosted ApplicationEngine). "
                    "Requires pinned engine env: network=0, address_version=53, "
                    "TriggerType=Application, persisting-block timestamp=1700000000."
                ),
                "generator": "scripts/gen-syscall-vectors.py",
                "baseline": "C# neo 3.10.1 / neo-rs",
                "vectors": vectors,
            },
            indent=1,
        )
        + "\n",
        encoding="utf-8",
    )
    groups: dict[str, int] = {}
    for v in vectors:
        groups[v["group"]] = groups.get(v["group"], 0) + 1
    print(f"wrote {OUT.relative_to(REPO)}: {len(vectors)} vectors")
    for g, n in sorted(groups.items()):
        print(f"  {g:12} {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
