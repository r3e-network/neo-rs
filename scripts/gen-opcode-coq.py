#!/usr/bin/env python3
"""Generate formal/coqlib/opcode_price_full_256.v from the Rust source of truth.

Reads neo-core/src/smart_contract/application_engine/op_code_prices.rs
`OPCODE_PRICE_TABLE: [i64; 256]`, and emits a self-contained Coq model that
represents the table as a `list nat` and proves, for every index in [0,256),
the exact per-opcode price via `nth` + reflexivity.  This keeps the Coq data
byte-identical to the Rust table (no hand transcription).

Model design (avoids the symbolic `if`-chain reduction problem that a
`nat -> nat` match on 256 cases would have):
  - opcode_price_all op := nth op price_table 0   (nat; out-of-range -> 0)
  - all_opcodes_non_negative : forall op, opcode_price_all op >= 0
        proven by nat positivity (0 <= nth op table 0), no table expansion.
  - opcode_table_size : length price_table = 256        by reflexivity
  - per_opcode_price i : opcode_price_all i = v_i       by simpl/reflexivity
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RUST = REPO / "neo-core" / "src" / "smart_contract" / "application_engine" / "op_code_prices.rs"
OUTS = [
    REPO / "formal" / "coqlib" / "opcode_price_full_256.v",
    REPO / "formal" / "coqlib" / "opcode_price_refinement.v",
]


def parse_table(src: str) -> list[int]:
    m = re.search(r"OPCODE_PRICE_TABLE:\s*\[i64;\s*256\]\s*=\s*\[(.*?)\];", src, re.S)
    if not m:
        raise SystemExit("OPCODE_PRICE_TABLE literal not found in Rust source")
    vals = [int(tok) for tok in re.findall(r"-?\d+", m.group(1))]
    if len(vals) != 256:
        raise SystemExit(f"expected 256 prices, got {len(vals)}")
    if any(v < 0 for v in vals):
        raise SystemExit("negative price in table — prices are execution units and must be >= 0")
    return vals


def fmt_table(vals: list[int]) -> str:
    n = len(vals)
    parts = []
    for i, v in enumerate(vals):
        sep = ";" if i < n - 1 else ""
        parts.append(f"{v}{sep}")
    per = 8
    rows = [" ".join(parts[i:i + per]) for i in range(0, len(parts), per)]
    return "  [" + "\n   ".join(rows) + "]"


def main() -> None:
    src = RUST.read_text(encoding="utf-8")
    vals = parse_table(src)

    # sanity spot-checks from the Rust file's own comment (known C# refs)
    expected = {0: 1, 12: 8, 33: 1, 0x41: 0, 0xC5: 512, 0xC8: 8192}
    for idx, exp in expected.items():
        if vals[idx] != exp:
            raise SystemExit(f"sanity mismatch: table[{idx}]={vals[idx]}, expected {exp}")

    body = []
    body.append("Require Import Arith Lia List.")
    body.append("Import ListNotations.")
    body.append("")
    body.append("(** Opcode execution-unit prices, generated verbatim from the Rust")
    body.append("    source of truth neo-core/src/smart_contract/application_engine/")
    body.append("    op_code_prices.rs OPCODE_PRICE_TABLE (indexed by OpCode byte value).")
    body.append("    Values are the pre-ExecFeeFactor execution-unit costs.  These are")
    body.append("    NOT claimed to match the C# reference beyond what the Rust source")
    body.append("    documents; a full automated C# cross-check remains TODO(M-17). *)")
    body.append("")
    body.append("Definition price_table : list nat :=")
    body.append(fmt_table(vals))
    body.append(".")
    body.append("")
    body.append("(** Price for an opcode byte value; out-of-range indexes cost 0. *)")
    body.append("Definition opcode_price_all (op : nat) : nat :=")
    body.append("  nth op price_table 0.")
    body.append("")
    body.append("(** Every opcode price is non-negative (a nat). *)")
    body.append("Lemma all_opcodes_non_negative :")
    body.append("  forall op : nat, opcode_price_all op >= 0.")
    body.append("Proof.")
    body.append("  intros op. unfold opcode_price_all. lia.")
    body.append("Qed.")
    body.append("")
    body.append("(** The table covers exactly the 256 opcode byte values. *)")
    body.append("Lemma opcode_table_size : length price_table = 256.")
    body.append("Proof. reflexivity. Qed.")
    body.append("")
    body.append("(** Per-opcode prices, machine-checked against the Rust table. *)")
    for i, v in enumerate(vals):
        body.append(f"Example price_{i:03d} : opcode_price_all {i} = {v}.")
        body.append("Proof. reflexivity. Qed.")
    body.append("")
    content = "\n".join(body) + "\n"

    for out in OUTS:
        out.write_text(content, encoding="utf-8")
    print(f"wrote {len(OUTS)} files ({len(vals)} prices, {len(vals)} per-opcode Examples each)")


if __name__ == "__main__":
    main()
