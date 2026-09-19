#!/usr/bin/env python3
"""Coq ↔ TLA+ 单一事实来源桥生成器。

输入：formal/bridge/constants.json（唯一来源，含 dBFT quorum 与 varint 语义常量）
输出：
  - formal/bridge/generated/coq_bridge_constants.v
  - formal/bridge/generated/tla_bridge_constants.tla
并且对每一组共享语义做**机器计算一致性校验**（f=(n-1)/3、M=n-f、varint 长度分档），
把校验结果写入 formal/bridge/generated/check.txt。

原则：只对"协议规格层的数值/常量语义"做桥——不主张程序级精化。Coq 侧只生成
可由权威门禁编译的自包含模块（无公理/admit），TLA+ 侧只生成常量定义片段。
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SRC = REPO / "formal" / "bridge" / "constants.json"
OUT_DIR = REPO / "formal" / "bridge" / "generated"


def main() -> None:
    data = json.loads(SRC.read_text(encoding="utf-8"))
    out = OUT_DIR
    out.mkdir(parents=True, exist_ok=True)

    checks: list[str] = []

    # ---- dBFT quorum: f=(n-1)/3, M=n-f，以 Rust context/mod.rs 为 spec ----
    n = int(data["dBFT"]["n"])
    f = (n - 1) // 3
    M = n - f
    rust_f = data["dBFT"]["expected_f"]
    rust_M = data["dBFT"]["expected_M"]
    checks.append(f"dBFT: n={n} f=(n-1)/3={f} (expect {rust_f}) M=n-f={M} (expect {rust_M}) ok={f==rust_f and M==rust_M}")
    assert f == rust_f and M == rust_M, "dBFT f/M 与 Rust spec 不一致"

    # ---- varint: 长度分档（与 varint_encoding Coq 模型一致） ----
    def varint_len(v: int) -> int:
        if v < 253:
            return 1
        if v <= 65535:
            return 3
        if v <= 4294967295:
            return 5
        return 9

    for entry in data["varint"]["samples"]:
        v = int(entry["value"])
        got = varint_len(v)
        expect = int(entry["len"])
        checks.append(f"varint: {v} -> {got} (expect {expect}) ok={got==expect}")
        assert got == expect, f"varint 长度与样本不一致 value={v}"

    # ---- 产出 Coq 常量模块 ----
    coq = []
    coq.append("(** dBFT quorum + varint 语义常量 — 由 formal/bridge/constants.json")
    coq.append("    单一事实来源生成器产出。仅供协议规格层对应，不做程序级精化。*)")
    coq.append("Require Import Arith Lia List.")
    coq.append("Import ListNotations.")
    coq.append("")
    coq.append("Definition DBFT_N : nat := %d." % n)
    coq.append("Definition DBFT_F : nat := %d.  (*(n-1)/3, 同 neo-consensus context::f*)" % f)
    coq.append("Definition DBFT_M : nat := DBFT_N - DBFT_F.  (*n-f, 同 context::m*)")
    coq.append("Lemma dbft_f_formula : DBFT_F = (DBFT_N - 1) / 3.")
    coq.append("Proof. reflexivity. Qed.")
    coq.append("Lemma dbft_m_formula : DBFT_M = DBFT_N - DBFT_F.")
    coq.append("Proof. reflexivity. Qed.")
    coq.append("Lemma dbft_quorum_positive :")
    coq.append("  DBFT_F < DBFT_N -> DBFT_M > DBFT_F.")
    coq.append("Proof. intros _. unfold DBFT_F, DBFT_N, DBFT_M. simpl. auto. Qed.")
    coq.append("")
    coq.append("Definition VAR_INT_U16_MARKER : nat := 253.")
    coq.append("Definition VAR_INT_U32_MARKER : nat := 254.")
    coq.append("Definition VAR_INT_U64_MARKER : nat := 255.")
    coq.append("Definition MAX_U16 : nat := 256 * 256 - 1.")
    coq.append("Definition MAX_U32 : nat := 256 * 256 * 256 * 256 - 1.")
    coq.append("")
    coq.append("Definition varint_len (value : nat) : nat :=")
    coq.append("  if Nat.ltb value VAR_INT_U16_MARKER then 1")
    coq.append("  else if Nat.leb value MAX_U16 then 3")
    coq.append("  else if Nat.leb value MAX_U32 then 5")
    coq.append("  else 9.")
    coq.append("")
    coq.append("Lemma varint_len_1  : varint_len 0   = 1.  Proof. reflexivity. Qed.")
    coq.append("Lemma varint_len_252: varint_len 252 = 1.  Proof. reflexivity. Qed.")
    coq.append("Lemma varint_len_253: varint_len 253 = 3.  Proof. reflexivity. Qed.")
    coq.append("(* Large-boundary samples are checked by the generator's Python arithmetic;")
    coq.append("   avoiding giant nat normalization in Coq 8.18. *)")
    coq.append("")
    (OUT_DIR / "coq_bridge_constants.v").write_text("\n".join(coq) + "\n", encoding="utf-8")

    # ---- 产出 TLA+ 常量片段 ----
    tla = []
    tla.append("\\\\ dBFT quorum + varint 语义常量 — 由 constants.json 生成器产出（协议规格层）")
    tla.append("\\\\ f = (n-1)/3、M = n-f（同 neo-consensus context），varint 长度分档")
    tla.append("N == %d" % n)
    tla.append("F == %d" % f)
    tla.append("M_ == %d" % M)
    tla.append("SafeQuorumBridge == (2 * M_ - N) > F")
    tla.append("")
    tla.append("VarintLen(v) == CASE v < 253 -> 1")
    tla.append("                  [] v <= 65535 -> 3")
    tla.append("                  [] v <= 4294967295 -> 5")
    tla.append("                  [] OTHER -> 9")
    (OUT_DIR / "tla_bridge_constants.tla").write_text("\n".join(tla) + "\n", encoding="utf-8")

    # ---- 一致性汇总 ----
    src_sha = hashlib.sha256(SRC.read_bytes()).hexdigest()
    checks.insert(0, f"source sha256: {src_sha}")
    check_txt = "\n".join(checks) + "\n"
    (OUT_DIR / "check.txt").write_text(check_txt, encoding="utf-8")
    print(check_txt)
    print(f"wrote: {OUT_DIR/'coq_bridge_constants.v'}, {OUT_DIR/'tla_bridge_constants.tla'}")


if __name__ == "__main__":
    main()
