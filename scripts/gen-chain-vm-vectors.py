#!/usr/bin/env python3
"""Extract REAL on-chain scripts from captured MainNet blocks as VM diff vectors.

The pure-VM vector set (``vectors/vm/diff-vectors.json``) is synthetic: it probes
opcode boundaries one at a time. This script produces the complementary set --
scripts that actually executed on MainNet inside real transactions. They exercise
the opcode *mixtures* that production contracts emit (NEP-17 transfers, native
contract calls, multi-sig checks), which no hand-written boundary probe covers.

Source
------
``neo-core/tests/protocol_compliance/test_vectors/mainnet_blocks.json`` -- 27 real
blocks captured from a live node, covering every hardfork activation height
(Aspidochelone .. Gorgon) plus a 512-transaction deep-merkle block.

What is extracted
-----------------
For each transaction in each block we take the two scripts a Neo node actually
feeds to the VM:

* **verification script** (``Witness.verification_script``) -- executed by
  ``CheckWitness`` / signature verification.
* **invocation script** (``Witness.invocation_script``) -- the pushed signature
  data that precedes it.

Both are emitted verbatim as hex. The diff harness runs them on a *bare* engine
with no host, so scripts containing ``SYSCALL`` will FAULT on both sides --
which is itself the assertion (the two VMs must agree on *where* they stop).

Output
------
``vectors/vm/chain-vectors.json`` in the same schema as ``diff-vectors.json``,
so the same two runners consume it unchanged.

Usage
-----
    python scripts/gen-chain-vm-vectors.py [--out vectors/vm/chain-vectors.json]
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
BLOCKS = (
    REPO
    / "neo-core/tests/protocol_compliance/test_vectors/mainnet_blocks.json"
)
DEFAULT_OUT = REPO / "vectors/vm/chain-vectors.json"


# --------------------------------------------------------------------------
# Minimal Neo N3 wire reader
# --------------------------------------------------------------------------


class Reader:
    """Cursor over a byte buffer with the varint/varbytes primitives Neo uses."""

    def __init__(self, data: bytes) -> None:
        self.data = data
        self.pos = 0

    def need(self, n: int) -> None:
        if self.pos + n > len(self.data):
            raise ValueError(
                f"truncated: want {n} bytes at {self.pos}, have {len(self.data) - self.pos}"
            )

    def u8(self) -> int:
        self.need(1)
        v = self.data[self.pos]
        self.pos += 1
        return v

    def u16(self) -> int:
        self.need(2)
        v = int.from_bytes(self.data[self.pos : self.pos + 2], "little")
        self.pos += 2
        return v

    def u32(self) -> int:
        self.need(4)
        v = int.from_bytes(self.data[self.pos : self.pos + 4], "little")
        self.pos += 4
        return v

    def u64(self) -> int:
        self.need(8)
        v = int.from_bytes(self.data[self.pos : self.pos + 8], "little")
        self.pos += 8
        return v

    def i64(self) -> int:
        self.need(8)
        v = int.from_bytes(self.data[self.pos : self.pos + 8], "little", signed=True)
        self.pos += 8
        return v

    def bytes(self, n: int) -> bytes:
        self.need(n)
        v = self.data[self.pos : self.pos + n]
        self.pos += n
        return v

    def varint(self) -> int:
        """Neo's variable-length integer. Note: for values < 0xFD it is 1 byte."""
        first = self.u8()
        if first < 0xFD:
            return first
        if first == 0xFD:
            return self.u16()
        if first == 0xFE:
            return self.u32()
        return self.u64()

    def varbytes(self) -> bytes:
        return self.bytes(self.varint())


def skip_witnesses(r: Reader) -> None:
    """Skip the witness list of a block header or transaction."""
    for _ in range(r.varint()):
        r.varbytes()  # invocation script
        r.varbytes()  # verification script


# WitnessScope flag bits (neo-primitives/src/witness_scope.rs)
SCOPE_CALLED_BY_ENTRY = 0x01
SCOPE_CUSTOM_CONTRACTS = 0x10
SCOPE_CUSTOM_GROUPS = 0x20
SCOPE_WITNESS_RULES = 0x40
SCOPE_GLOBAL = 0x80


def skip_signer(r: Reader) -> None:
    """Skip one Signer.

    Layout: account:20, scopes:u8, then conditionally:
      CustomContracts -> varint(count) * 20 bytes
      CustomGroups    -> varint(count) * 33 bytes (compressed secp256r1 point)
      WitnessRules    -> varint(count) * WitnessRule

    ``GLOBAL`` (0x80) carries **no** trailing payload -- treating it as "has
    sub-items" desynchronises the cursor.
    """
    r.bytes(20)  # account
    scopes = r.u8()

    if scopes & SCOPE_CUSTOM_CONTRACTS:
        for _ in range(r.varint()):
            r.bytes(20)
    if scopes & SCOPE_CUSTOM_GROUPS:
        for _ in range(r.varint()):
            r.bytes(33)
    if scopes & SCOPE_WITNESS_RULES:
        for _ in range(r.varint()):
            skip_witness_rule(r)


def skip_witness_condition(r: Reader) -> None:
    """Skip one WitnessCondition (a tagged union, 0x00..0x24)."""
    tag = r.u8()
    if tag == 0x00:  # Boolean
        r.u8()
    elif tag == 0x01:  # Not
        skip_witness_condition(r)
    elif tag in (0x02, 0x03):  # And / Or
        for _ in range(r.u8()):
            skip_witness_condition(r)
    elif tag == 0x20:  # ScriptHash
        r.bytes(20)
    elif tag == 0x21:  # Group
        r.bytes(33)
    elif tag == 0x22:  # CalledByEntry -- no payload
        pass
    elif tag == 0x23:  # CalledByContract
        r.bytes(20)
    elif tag == 0x24:  # CalledByGroup
        r.bytes(33)
    else:
        raise ValueError(f"unknown witness condition tag 0x{tag:02x} at {r.pos - 1}")


def skip_witness_rule(r: Reader) -> None:
    """Skip one WitnessRule: 1-byte action + condition."""
    r.u8()  # action (Deny/Allow)
    skip_witness_condition(r)


def skip_attrs(r: Reader) -> None:
    """Skip Neo N3 transaction attributes by their typed wire payload.

    Attributes are *not* generic varbytes.  This mirrors
    ``TransactionAttribute::deserialize_from`` in neo-core.
    """
    for _ in range(r.varint()):
        attr_type = r.u8()
        if attr_type == 0x01:  # HighPriority: unit attribute
            continue
        if attr_type == 0x11:  # OracleResponse: id:u64, code:u8, result:varbytes
            r.u64()
            r.u8()
            r.varbytes()
            continue
        if attr_type == 0x20:  # NotValidBefore: height:u32
            r.u32()
            continue
        if attr_type == 0x21:  # Conflicts: UInt256
            r.bytes(32)
            continue
        if attr_type == 0x22:  # NotaryAssisted: nkeys:u8
            r.u8()
            continue
        raise ValueError(f"unknown transaction attribute type 0x{attr_type:02x}")


def skip_header_prefix(r: Reader) -> None:
    """The part of the header before the witness list.

    Layout (matches ``neo-core/src/ledger/block_header.rs``):
    version:u32, prev:32, merkle:32, timestamp:u64, nonce:u64, index:u32,
    primary_index:u8, next_consensus:20.

    Note timestamp and nonce are **u64** -- reading them as u32 desynchronises
    the cursor and makes the parser fail ~40 bytes later inside the witness list.
    """
    r.u32()  # version
    r.bytes(32)  # prev hash
    r.bytes(32)  # merkle root
    r.u64()  # timestamp (ms since epoch)
    r.u64()  # nonce
    r.u32()  # index
    r.u8()  # primary index
    r.bytes(20)  # next consensus


def skip_tx(r: Reader) -> None:
    """Skip the body of one transaction, up to (not including) its witnesses."""
    r.u8()  # version
    r.u32()  # nonce
    r.i64()  # system fee
    r.i64()  # network fee
    r.u32()  # valid until block
    for _ in range(r.varint()):  # signers
        skip_signer(r)
    skip_attrs(r)
    r.varbytes()  # script


def extract_witnesses(r: Reader, out: list[tuple[bytes, bytes]]) -> None:
    for _ in range(r.varint()):
        inv = r.varbytes()
        ver = r.varbytes()
        out.append((inv, ver))


def parse_block(block_hex: str) -> list[tuple[bytes, bytes]]:
    """Return every (invocation, verification) witness pair in the block.

    A block is ``header || varint(txcount) || tx*`` where each header ends in a
    witness list, and each transaction ends in its own witness list.
    """
    r = Reader(bytes.fromhex(block_hex))
    out: list[tuple[bytes, bytes]] = []

    skip_header_prefix(r)
    extract_witnesses(r, out)  # the header's primary witness

    tx_count = r.varint()
    for _ in range(tx_count):
        skip_tx(r)
        extract_witnesses(r, out)

    if r.pos != len(r.data):
        raise ValueError(
            f"trailing bytes: consumed {r.pos}, buffer is {len(r.data)}"
        )
    return out


# --------------------------------------------------------------------------


def hexs(b: bytes) -> str:
    return b.hex()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)
    args = ap.parse_args()

    data = json.loads(BLOCKS.read_text(encoding="utf-8"))
    vectors: list[dict] = []
    seen: set[str] = set()
    stats = {"blocks": 0, "tx_witnesses": 0, "header_witnesses": 0, "empty": 0}

    for blk in data["blocks"]:
        height = blk["height"]
        try:
            pairs = parse_block(blk["block_hex"])
        except ValueError as exc:
            print(f"  !! height {height}: parse failed: {exc}", file=sys.stderr)
            return 1
        stats["blocks"] += 1

        for idx, (inv, ver) in enumerate(pairs):
            # The header witness is the first pair of the block; transactions follow.
            kind = "header" if idx == 0 else "tx"
            if kind == "header":
                stats["header_witnesses"] += 1
            else:
                stats["tx_witnesses"] += 1

            for role, script in (("verif", ver), ("invoc", inv)):
                if not script:
                    stats["empty"] += 1
                    continue
                key = script.hex()
                if key in seen:
                    continue
                seen.add(key)
                name = f"chain.{height}.{kind}{idx}.{role}"
                vectors.append(
                    {
                        "name": name,
                        "group": "chain-real-scripts",
                        "note": f"height {height}: {blk.get('note', '')}".strip(),
                        "script": hexs(script),
                    }
                )

    out = {
        "generated_by": "scripts/gen-chain-vm-vectors.py",
        "source": str(BLOCKS.relative_to(REPO)).replace("\\", "/"),
        "network": data.get("network"),
        "chain_tip": data.get("chain_tip"),
        "stats": stats,
        "vectors": vectors,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(out, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"blocks parsed      : {stats['blocks']}")
    print(f"header witnesses   : {stats['header_witnesses']}")
    print(f"tx witnesses       : {stats['tx_witnesses']}")
    print(f"empty scripts skip : {stats['empty']}")
    print(f"unique vectors out : {len(vectors)}")
    try:
        display_out = args.out.resolve().relative_to(REPO)
    except ValueError:
        display_out = args.out.resolve()
    print(f"written            : {display_out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
