#!/usr/bin/env python3
"""Generate VM differential-execution vectors.

Produces `vectors/vm/diff-vectors.json`, consumed by BOTH:
  - Rust  : neo-vm/examples/vm_diff_runner.rs
  - C#    : tools/csharp-vm-runner (Neo.VM 3.10.1)

Design rules
------------
1. Opcode bytes are read from the Rust source of truth (`neo-vm/src/vm/opcode.rs`)
   rather than hardcoded, so a byte-value drift cannot silently desync the
   vectors from the implementation under test.
2. Vectors avoid `RET`/`INITSLOT`: scripts simply run off the end, which HALTs
   with the accumulator on the result stack. `INITSLOT 0,0` is rejected by both
   implementations, and `RET` with rvcount=-1 requires initialized locals.
3. Vectors are pure-VM: no syscalls, no host, no storage. This is the
   controllable surface where a divergence is unambiguous.

Usage:
  python scripts/gen-vm-diff-vectors.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OPCODE_RS = REPO / "neo-vm" / "src" / "vm" / "opcode.rs"
OUT = REPO / "vectors" / "vm" / "diff-vectors.json"

# C# Neo.VM `StackItemType` tag values, used as immediate operands by
# NEWARRAY_T / CONVERT. Kept as literals because they are part of the wire
# format, not an implementation detail of either side.
NEOVM_TYPE_INTEGER = 0x21
NEOVM_TYPE_BYTESTRING = 0x28


def load_opcodes() -> dict[str, int]:
    """Extract `NAME = 0xNN` pairs from the Rust opcode enum."""
    src = OPCODE_RS.read_text(encoding="utf-8")
    table: dict[str, int] = {}
    for m in re.finditer(r"^\s*([A-Z][A-Z0-9_]*)\s*=\s*(0x[0-9A-Fa-f]+)\s*,", src, re.M):
        table[m.group(1)] = int(m.group(2), 16)
    if not table:
        raise SystemExit(f"no opcodes parsed from {OPCODE_RS}")
    return table


class Builder:
    """Assembles NeoVM bytecode from named opcodes."""

    def __init__(self, ops: dict[str, int]):
        self.ops = ops
        self.buf: list[int] = []

    def op(self, name: str) -> "Builder":
        self.buf.append(self.ops[name])
        return self

    def push(self, value: int) -> "Builder":
        """Emit the shortest PUSH for a small integer; PUSHINT* otherwise."""
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

    def bytes_(self, data: bytes) -> "Builder":
        """Emit PUSHDATA1 for arbitrary-length byte strings."""
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

    def jump(self, name: str, target: int) -> "Builder":
        """Emit a short jump whose operand is relative to the post-operand IP.

        `execute_jump_offset` adds the offset to the instruction pointer *after*
        the operand byte, so target - (operand_index + 1) is the correct delta.
        """
        operand_index = len(self.buf) + 1  # +1 for the opcode itself
        delta = target - (operand_index + 1)
        if not -128 <= delta <= 127:
            raise ValueError(f"{name}: short jump delta {delta} out of range")
        self.buf.append(self.ops[name])
        self.buf.append(delta & 0xFF)
        return self

    @property
    def pos(self) -> int:
        return len(self.buf)

    def op_byte(self, name: str) -> "Builder":
        """Emit an opcode as a *literal operand byte* (e.g. NEWARRAY_T's type)."""
        self.buf.append(self.ops[name])
        return self

    def raw_byte(self, value: int) -> "Builder":
        """Emit an arbitrary immediate operand byte (e.g. a StackItemType tag)."""
        self.buf.append(value & 0xFF)
        return self

    def hex(self) -> str:
        return "".join(f"{b:02x}" for b in self.buf)


def build_vectors(ops: dict[str, int]) -> list[dict]:
    vectors: list[dict] = []

    def add(name: str, group: str, b: Builder, note: str = "") -> None:
        vectors.append(
            {
                "name": name,
                "group": group,
                "note": note,
                "script": b.hex(),
            }
        )

    # ---- Arithmetic: basic + sign handling -------------------------------
    add("arith.add.basic", "arithmetic", Builder(ops).push(1).push(2).op("ADD"))
    add("arith.sub.negative", "arithmetic", Builder(ops).push(1).push(2).op("SUB"))
    add("arith.mul.basic", "arithmetic", Builder(ops).push(6).push(7).op("MUL"))
    add("arith.div.trunc.pos", "arithmetic", Builder(ops).push(7).push(2).op("DIV"))
    add("arith.div.trunc.neg", "arithmetic", Builder(ops).push(-7).push(2).op("DIV"),
        "C# and Rust must agree on truncation direction for negative operands")
    add("arith.mod.neg", "arithmetic", Builder(ops).push(-7).push(2).op("MOD"),
        "MOD sign rule is a classic cross-implementation divergence")
    add("arith.mod.pos", "arithmetic", Builder(ops).push(7).push(2).op("MOD"))
    add("arith.negate", "arithmetic", Builder(ops).push(42).op("NEGATE"))
    add("arith.abs.negative", "arithmetic", Builder(ops).push(-42).op("ABS"))
    add("arith.inc.max", "arithmetic", Builder(ops).push((2**63) - 1).op("INC"),
        "increment past i64::MAX: fault or promote to bigint?")
    add("arith.dec.min", "arithmetic", Builder(ops).push(-(2**63)).op("DEC"),
        "decrement past i64::MIN: fault or promote to bigint?")

    # ---- Arithmetic: overflow boundaries ---------------------------------
    add("arith.add.overflow.i64", "arith-overflow",
        Builder(ops).push((2**63) - 1).push(1).op("ADD"))
    add("arith.add.underflow.i64", "arith-overflow",
        Builder(ops).push(-(2**63)).push(-1).op("ADD"))
    add("arith.mul.overflow.i64", "arith-overflow",
        Builder(ops).push((2**62)).push(4).op("MUL"))
    add("arith.div.min.neg1", "arith-overflow",
        Builder(ops).push(-(2**63)).push(-1).op("DIV"),
        "i64::MIN / -1 overflows two's complement")

    # ---- Division by zero ------------------------------------------------
    add("arith.div.by.zero", "divzero", Builder(ops).push(1).push(0).op("DIV"))
    add("arith.mod.by.zero", "divzero", Builder(ops).push(1).push(0).op("MOD"))

    # ---- Comparison ------------------------------------------------------
    for op, desc in (("EQUAL", "eq"), ("NUMNOTEQUAL", "ne"), ("LT", "lt"),
                     ("LE", "le"), ("GT", "gt"), ("GE", "ge")):
        add(f"cmp.{desc}.true", "compare", Builder(ops).push(1).push(2).op(op))
        add(f"cmp.{desc}.false", "compare", Builder(ops).push(2).push(1).op(op))
        add(f"cmp.{desc}.equal", "compare", Builder(ops).push(2).push(2).op(op))
    add("cmp.within.true", "compare", Builder(ops).push(5).push(1).push(10).op("WITHIN"))
    add("cmp.within.false.lo", "compare", Builder(ops).push(0).push(1).push(10).op("WITHIN"))
    add("cmp.within.false.hi", "compare", Builder(ops).push(11).push(1).push(10).op("WITHIN"))
    add("cmp.sign.neg", "compare", Builder(ops).push(-5).op("SIGN"))
    add("cmp.sign.zero", "compare", Builder(ops).push(0).op("SIGN"))
    add("cmp.sign.pos", "compare", Builder(ops).push(5).op("SIGN"))
    add("cmp.max", "compare", Builder(ops).push(3).push(9).op("MAX"))
    add("cmp.min", "compare", Builder(ops).push(3).push(9).op("MIN"))

    # ---- Bitwise / shift boundaries --------------------------------------
    add("bit.and", "bitwise", Builder(ops).push(0b1100).push(0b1010).op("AND"))
    add("bit.or", "bitwise", Builder(ops).push(0b1100).push(0b1010).op("OR"))
    add("bit.xor", "bitwise", Builder(ops).push(0b1100).push(0b1010).op("XOR"))
    add("bit.not.zero", "bitwise", Builder(ops).push(0).op("NOT"))
    add("bit.not.minus1", "bitwise", Builder(ops).push(-1).op("NOT"))
    add("bit.shl.zero", "bitwise", Builder(ops).push(1).push(0).op("SHL"))
    add("bit.shl.255", "bitwise", Builder(ops).push(1).push(255).op("SHL"),
        "MaxShift boundary is 256")
    add("bit.shl.256", "bitwise", Builder(ops).push(1).push(256).op("SHL"),
        "shift == MaxShift: boundary behavior")
    add("bit.shl.257", "bitwise", Builder(ops).push(1).push(257).op("SHL"),
        "shift > MaxShift: fault expected")
    add("bit.shl.negative", "bitwise", Builder(ops).push(1).push(-1).op("SHL"),
        "negative shift: fault expected")
    add("bit.shr.positive", "bitwise", Builder(ops).push(256).push(4).op("SHR"),
        "SHR must be arithmetic (sign-preserving) on both sides")
    add("bit.shr.negative", "bitwise", Builder(ops).push(-256).push(4).op("SHR"))
    add("bit.shr.256", "bitwise", Builder(ops).push(1).push(256).op("SHR"))
    add("bit.shr.257", "bitwise", Builder(ops).push(1).push(257).op("SHR"))
    add("bit.packmap", "bitwise",
        Builder(ops).push(1).push(1).push(1).op("PACKMAP"),
        "PACKMAP consumes 2n items from the stack; n=1 pair here")

    # ---- Stack manipulation ---------------------------------------------
    add("stack.depth", "stack", Builder(ops).push(1).push(2).op("DEPTH"))
    add("stack.dup", "stack", Builder(ops).push(7).op("DUP"))
    add("stack.drop", "stack", Builder(ops).push(1).push(2).op("DROP"))
    add("stack.swap", "stack", Builder(ops).push(1).push(2).op("SWAP"))
    add("stack.over", "stack", Builder(ops).push(1).push(2).op("OVER"))
    add("stack.nip", "stack", Builder(ops).push(1).push(2).op("NIP"))
    add("stack.tuck", "stack", Builder(ops).push(1).push(2).op("TUCK"))
    add("stack.rot", "stack", Builder(ops).push(1).push(2).push(3).op("ROT"))
    add("stack.reverse3", "stack",
        Builder(ops).push(1).push(2).push(3).op("REVERSE3"))
    add("stack.reverse4", "stack",
        Builder(ops).push(1).push(2).push(3).push(4).op("REVERSE4"))
    add("stack.reversen", "stack",
        Builder(ops).push(1).push(2).push(3).push(4).push(4).op("REVERSEN"))
    add("stack.pick", "stack", Builder(ops).push(10).push(20).push(0).op("PICK"))
    add("stack.roll", "stack", Builder(ops).push(10).push(20).push(0).op("ROLL"))
    add("stack.dup.empty", "stack-errors", Builder(ops).op("DUP"),
        "DUP on empty stack: underflow")
    add("stack.drop.empty", "stack-errors", Builder(ops).op("DROP"))
    add("stack.swap.one", "stack-errors", Builder(ops).push(1).op("SWAP"),
        "SWAP with one item: underflow")
    add("stack.pick.oob", "stack-errors", Builder(ops).push(1).push(5).op("PICK"))

    # ---- Control flow ----------------------------------------------------
    add("flow.nop", "control", Builder(ops).op("NOP").push(1))

    # JMP forward over two PUSH1s, landing on PUSH2.
    # After `jump()` writes opcode+operand, buffer length == operand_index+1,
    # so the byte right after the operand is at index `b.pos` post-call.
    b = Builder(ops)
    skip = [ops["PUSH1"], ops["PUSH1"]]
    b.jump("JMP", b.pos + 2 + len(skip))
    b.buf += skip
    b.push(2)
    add("flow.jmp.forward", "control", b, "jump over two PUSH1s onto PUSH2")

    # JMPIF taken: PUSH1 at 0, JMPIF at 1, operand at 2.
    b = Builder(ops).push(1)                      # pos now 1 (JMPIF goes at 1)
    skip = [ops["PUSH1"], ops["PUSH1"]]
    b.jump("JMPIF", b.pos + 2 + len(skip))
    b.buf += skip
    b.push(9).push(1).op("ADD")
    add("flow.jmpif.taken", "control", b)

    # JMPIF not taken: falls straight through to the next instruction.
    b = Builder(ops).push(0)
    b.jump("JMPIF", b.pos + 2)
    b.push(7)
    add("flow.jmpif.not.taken", "control", b)

    # JMPIFNOT taken when the condition is falsy: skip one 1-byte PUSH.
    b = Builder(ops).push(0)
    b.jump("JMPIFNOT", b.pos + 2 + 1)
    b.buf.append(ops["PUSH1"])
    b.push(8).push(1).op("ADD")
    add("flow.jmpifnot.taken", "control", b)

    add("flow.assert.ok", "control", Builder(ops).push(1).op("ASSERT").push(5))
    add("flow.assert.fail", "control", Builder(ops).push(0).op("ASSERT"))

    # ---- Compound types --------------------------------------------------
    add("type.newarray0", "compound", Builder(ops).op("NEWARRAY0"))
    add("type.newarray_t.int", "compound",
        Builder(ops).push(3).op("NEWARRAY_T").raw_byte(NEOVM_TYPE_INTEGER),
        "NEWARRAY_T takes its type as an immediate operand (0x21 = Integer)")
    add("type.pack", "compound",
        Builder(ops).push(1).push(2).push(2).op("PACK"))
    add("type.unpack", "compound",
        Builder(ops).push(1).push(2).push(2).op("PACK").op("UNPACK"))
    add("type.size.array", "compound",
        Builder(ops).push(1).push(2).push(2).op("PACK").op("SIZE"))
    add("type.newstruct0", "compound", Builder(ops).op("NEWSTRUCT0"))

    # ---- Splice / byte-string ops ---------------------------------------
    add("bytes.cat", "bytes", Builder(ops).bytes_(b"neo").bytes_(b"n4").op("CAT"))
    add("bytes.left", "bytes", Builder(ops).bytes_(b"hello").push(3).op("LEFT"))
    add("bytes.right", "bytes", Builder(ops).bytes_(b"hello").push(3).op("RIGHT"))
    add("bytes.substr.inrange", "bytes",
        Builder(ops).bytes_(b"hello").push(1).push(3).op("SUBSTR"))
    add("bytes.substr.oob", "bytes-errors",
        Builder(ops).bytes_(b"hello").push(4).push(5).op("SUBSTR"))
    add("bytes.left.oob", "bytes-errors",
        Builder(ops).bytes_(b"hello").push(9).op("LEFT"))
    add("bytes.size", "bytes", Builder(ops).bytes_(b"hello").op("SIZE"))

    # ---- Integer <-> byte-string conversion ------------------------------
    # CONVERT takes its target type as an immediate operand byte.
    add("conv.int.to.bytestring", "convert",
        Builder(ops).push(128).op("CONVERT").raw_byte(NEOVM_TYPE_BYTESTRING),
        "CONVERT takes the target type as an immediate operand (0x28 = ByteString)")
    add("conv.bool.to.int", "convert",
        Builder(ops).push(1).op("NOT").op("NOT"))

    # ---- Empty / minimal scripts ----------------------------------------
    add("meta.empty", "meta", Builder(ops))
    add("meta.nop.only", "meta", Builder(ops).op("NOP"))

    return vectors


def main() -> int:
    ops = load_opcodes()
    vectors = build_vectors(ops)
    # Uniqueness guard: a duplicate name would silently mask a vector.
    seen: set[str] = set()
    for v in vectors:
        if v["name"] in seen:
            raise SystemExit(f"duplicate vector name: {v['name']}")
        seen.add(v["name"])

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        json.dumps(
            {
                "description": "NeoVM differential-execution vectors (pure VM, no host)",
                "generator": "scripts/gen-vm-diff-vectors.py",
                "baseline": "C# neo.vm 3.10.1 / neo-rs v0.17.0",
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
        print(f"  {g:16} {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
