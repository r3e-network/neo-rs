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

# Canonical mainnet native-contract hashes, shared verbatim by the C# reference
# (Neo 3.10.1) and the neo-rs native registry. Verified in
# `neo-core/tests/native_contract_tests.rs` (NeoToken id=-5) and the repo's
# offline comparison scripts. Both engines route System.Contract.Call to the
# same native contract when handed these bytes, so we embed the same bytes in
# every vector consumed by both runners.
NEO_HASH = bytes.fromhex("ef4073a0f2b305a38ec4050e4d3d28bc40ea63f5")
GAS_HASH = bytes.fromhex("d2a4cff31913016155e38e474a2c06d08be276cf")

# CallFlags.All = READ_STATES(0x01) | WRITE_STATES(0x02) | ALLOW_CALL(0x04)
# | ALLOW_NOTIFY(0x08) = 0x0F, matching neo-primitives CallFlags and C#.
CALL_FLAGS_ALL = 0x0F

# Signer account bytes used by the signer-scoped vectors. Both runners build the
# transaction container with a Signer whose account == these bytes, and the
# CheckWitness vectors pass the SAME bytes, so CheckWitness -> true. This is the
# standard account hash of VALID_PUBKEY (SignatureContract script hash), a
# recognizable value, but any 20 fixed bytes would do. Bytes are the internal
# (little-endian) form that both UInt160::from_bytes / UInt160.ToUInt160 use.
SIGNER_ACCOUNT = bytes.fromhex("c44ea575c5f79638f0e73f39d7bd4b3337c81691")


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

    def push_str(self, text: str) -> "Builder":
        """Push a UTF-8 string as a byte array (matches ScriptBuilder.EmitPush(string))."""
        return self.push_bytes(text.encode("utf-8"))

    def syscall(self, name: str) -> "Builder":
        self.buf.append(self.ops["SYSCALL"])
        self.buf += syscall_bytes(name)
        return self

    def contract_call(self, hash_bytes: bytes, method: str, flags: int = CALL_FLAGS_ALL) -> "Builder":
        """Emit System.Contract.Call(hash, method, flags, args=[]).

        C#/Rust pop in declaration order (hash -> method -> flags -> args), so
        the push order is: args, flags, method, hash (hash on top).

        `hash_bytes` is the *canonical display* hex (e.g. "ef4073..."). The VM
        interop reconstructs a UInt160 from raw little-endian internal bytes, so
        we push the reversed (internal) byte order — the same bytes both engines
        (neo-rs `UInt160::from_bytes` and C# `UInt160.ToUInt160`) interpret.
        """
        self.pack_array([])
        self.push_int(flags)
        self.push_bytes(method.encode("utf-8"))
        self.push_bytes(hash_bytes[::-1])  # internal byte order
        self.syscall("System.Contract.Call")
        return self

    def hex(self) -> str:
        return "".join(f"{b:02x}" for b in self.buf)


def build_vectors(ops: dict[str, int]) -> list[dict]:
    vectors: list[dict] = []

    def add(name: str, group: str, b: Builder, note: str = "", **extra) -> None:
        vectors.append(
            {"name": name, "group": group, "note": note, "script": b.hex(), **extra}
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

    # ---- CheckWitness positive path (container has SIGNS with the account) ----
    add("cw.positive_hash", "witness",
        Builder(ops).push_bytes(SIGNER_ACCOUNT)
        .syscall("System.Runtime.CheckWitness").op("RET"),
        "signer-derived 20-byte hash -> true (signer present in tx)",
        signer=SIGNER_ACCOUNT.hex())
    add("cw.positive_pubkey", "witness",
        Builder(ops).push_bytes(VALID_PUBKEY)
        .syscall("System.Runtime.CheckWitness").op("RET"),
        "CheckWitness(33-byte VALID_PUBKEY) -> true (converts to signer account)",
        signer=SIGNER_ACCOUNT.hex())
    add("runtime.current_signers_signed", "runtime",
        Builder(ops).syscall("System.Runtime.CurrentSigners").op("RET"),
        "CurrentSigners with one signer -> [{account, scopes}]",
        signer=SIGNER_ACCOUNT.hex())

    # ---- Runtime: gas / random / container / signers / log / notify ------
    add("runtime.get_random", "runtime",
        Builder(ops).syscall("System.Runtime.GetRandom").op("RET"),
        "GetRandom -> Integer (network=0, tx/block nonce-derived)")
    add("runtime.get_script_container", "runtime",
        Builder(ops).syscall("System.Runtime.GetScriptContainer").op("RET"),
        "GetScriptContainer -> transaction stack map")
    add("runtime.current_signers", "runtime",
        Builder(ops).syscall("System.Runtime.CurrentSigners").op("RET"),
        "CurrentSigners -> [] (minimal tx has no signers)")
    add("runtime.gas_left", "runtime",
        Builder(ops).syscall("System.Runtime.GasLeft").op("RET"),
        "GasLeft -> remaining datoshi after minimal script")
    add("runtime.gas_left_after_arith", "runtime",
        Builder(ops).push_int(1).push_int(2).op("ADD")
        .syscall("System.Runtime.GasLeft").op("RET"),
        "GasLeft after PUSH1 PUSH2 ADD -> exact datoshi consumed by arithmetic")
    add("runtime.log", "runtime",
        Builder(ops).push_bytes(b"hello")
        .syscall("System.Runtime.Log").op("PUSH1").op("RET"),
        "Log('hello') in dynamic script -> HALT [Integer 1]")
    add("runtime.notify", "runtime",
        Builder(ops).push_int(1).push_int(1).op("PACK")
        .push_str("Evt")
        .syscall("System.Runtime.Notify").op("RET"),
        "Notify in dynamic script -> FAULT (notifications disallowed in dynamic scripts)")

    # ---- System.Contract.Call to NEO/GAS native read-only constants ------
    add("native.neo_symbol", "native",
        Builder(ops).contract_call(NEO_HASH, "symbol").op("RET"),
        "NEO.symbol() -> 'NEO' ByteString")
    add("native.neo_decimals", "native",
        Builder(ops).contract_call(NEO_HASH, "decimals").op("RET"),
        "NEO.decimals() -> Integer 0")
    add("native.gas_symbol", "native",
        Builder(ops).contract_call(GAS_HASH, "symbol").op("RET"),
        "GAS.symbol() -> 'GAS' ByteString")
    add("native.gas_decimals", "native",
        Builder(ops).contract_call(GAS_HASH, "decimals").op("RET"),
        "GAS.decimals() -> Integer 8")

    # ---- System.Contract: account-hash helpers & call flags ---------------
    add("contract.create_standard_account", "contract",
        Builder(ops).push_bytes(VALID_PUBKEY)
        .syscall("System.Contract.CreateStandardAccount").op("RET"),
        "CreateStandardAccount(VALID_PUBKEY) -> 20-byte account hash")
    add("contract.create_multisig_account", "contract",
        Builder(ops).pack_array([VALID_PUBKEY, VALID_PUBKEY]).push_int(2)
        .syscall("System.Contract.CreateMultisigAccount").op("RET"),
        "CreateMultisigAccount(2, [pk,pk]) -> 20-byte multisig hash")
    add("contract.get_call_flags", "contract",
        Builder(ops).syscall("System.Contract.GetCallFlags").op("RET"),
        "GetCallFlags -> Integer 15 (entry context LoadScript CallFlags.All on both sides)")

    # ---- System.Storage: negative path (no deployed contract context) -----
    add("storage.get_context", "storage",
        Builder(ops).syscall("System.Storage.GetContext").op("RET"),
        "GetContext in bare dynamic script -> returns null (no contract context)")

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
