(** dBFT quorum + varint 语义常量 — 由 formal/bridge/constants.json
    单一事实来源生成器产出。仅供协议规格层对应，不做程序级精化。*)
Require Import Arith Lia List.
Import ListNotations.

Definition DBFT_N : nat := 7.
Definition DBFT_F : nat := 2.  (*(n-1)/3, 同 neo-consensus context::f*)
Definition DBFT_M : nat := DBFT_N - DBFT_F.  (*n-f, 同 context::m*)
Lemma dbft_f_formula : DBFT_F = (DBFT_N - 1) / 3.
Proof. reflexivity. Qed.
Lemma dbft_m_formula : DBFT_M = DBFT_N - DBFT_F.
Proof. reflexivity. Qed.
Lemma dbft_quorum_positive :
  DBFT_F < DBFT_N -> DBFT_M > DBFT_F.
Proof. intros _. unfold DBFT_F, DBFT_N, DBFT_M. simpl. auto. Qed.

Definition VAR_INT_U16_MARKER : nat := 253.
Definition VAR_INT_U32_MARKER : nat := 254.
Definition VAR_INT_U64_MARKER : nat := 255.
Definition MAX_U16 : nat := 256 * 256 - 1.
Definition MAX_U32 : nat := 256 * 256 * 256 * 256 - 1.

Definition varint_len (value : nat) : nat :=
  if Nat.ltb value VAR_INT_U16_MARKER then 1
  else if Nat.leb value MAX_U16 then 3
  else if Nat.leb value MAX_U32 then 5
  else 9.

Lemma varint_len_1  : varint_len 0   = 1.  Proof. reflexivity. Qed.
Lemma varint_len_252: varint_len 252 = 1.  Proof. reflexivity. Qed.
Lemma varint_len_253: varint_len 253 = 3.  Proof. reflexivity. Qed.
(* Large-boundary samples are checked by the generator's Python arithmetic;
   avoiding giant nat normalization in Coq 8.18. *)

