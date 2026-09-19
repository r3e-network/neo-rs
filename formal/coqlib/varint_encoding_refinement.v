Require Import Arith Lia List.
Import ListNotations.

(** Neo N3 VarInt encoding — honest length / round-trip model.

    Mirrors the wire rules in neo-io/src/var_int.rs:
      value <  253                 -> 1 byte  [value]
      253 <= value <=    65535     -> 3 bytes [0xFD, u16 LE]
      65536 <= value <= 4294967295 -> 5 bytes [0xFE, u32 LE]
      value >  4294967295          -> 9 bytes [0xFF, u64 LE]

    This is an abstract byte-list model (bytes are [nat]).  It is NOT a
    refinement proof against the Rust byte buffer.  We prove the properties
    that genuinely hold; we do not admit u64 bit-width reconstruction.
*)

Definition VAR_INT_U16_MARKER : nat := 253.
Definition VAR_INT_U32_MARKER : nat := 254.
Definition VAR_INT_U64_MARKER : nat := 255.
(** Bounds written as arithmetic (256^2-1 etc.) so `lia` can handle them;
    Coq's `of_num_uint` big literals defeat `lia`/`vm_compute`. *)
Definition MAX_U16 : nat := 256 * 256 - 1.
Definition MAX_U32 : nat := 256 * 256 * 256 * 256 - 1.

Section VarIntEncoding.

(** Number of bytes produced by encoding, per the table above. *)
Definition encoded_len (value : nat) : nat :=
  if Nat.ltb value VAR_INT_U16_MARKER then 1
  else if Nat.leb value MAX_U16 then 3
  else if Nat.leb value MAX_U32 then 5
  else 9.

(** Encode a value as a list of bytes. *)
Definition write_var_int (value : nat) : list nat :=
  if Nat.ltb value VAR_INT_U16_MARKER then
    [value]
  else if Nat.leb value MAX_U16 then
    VAR_INT_U16_MARKER :: (value mod 256) :: ((value / 256) mod 256) :: []
  else if Nat.leb value MAX_U32 then
    VAR_INT_U32_MARKER ::
      (value mod 256) ::
      ((value / 256) mod 256) ::
      (((value / 256) / 256) mod 256) ::
      ((((value / 256) / 256) / 256) mod 256) :: []
  else
    VAR_INT_U64_MARKER ::
      (value mod 256) ::
      ((value / 256) mod 256) ::
      (((value / 256) / 256) mod 256) ::
      ((((value / 256) / 256) / 256) mod 256) ::
      (((((value / 256) / 256) / 256) / 256) mod 256) ::
      ((((((value / 256) / 256) / 256) / 256) / 256) mod 256) ::
      (((((((value / 256) / 256) / 256) / 256) / 256) / 256) mod 256) ::
      ((((((((value / 256) / 256) / 256) / 256) / 256) / 256) / 256) mod 256) ::
      [].

(** Decode one VarInt from a buffer. Returns (value, len) or None. *)
Definition read_var_int_prefix (buf : list nat) : option (nat * nat) :=
  match buf with
  | [] => None
  | prefix :: rest =>
      if Nat.ltb prefix VAR_INT_U16_MARKER then
        Some (prefix, 1)
      else if Nat.eqb prefix VAR_INT_U16_MARKER then
        match rest with
        | b1 :: b2 :: _ => Some ((b1 + 256 * b2), 3)
        | _ => None
        end
      else if Nat.eqb prefix VAR_INT_U32_MARKER then
        match rest with
        | b1 :: b2 :: b3 :: b4 :: _ =>
            Some (b1 + 256 * b2 + 65536 * b3 + 16777216 * b4, 5)
        | _ => None
        end
      else
        None
  end.

(** ---- Length conservation per range ---- *)
Lemma encoded_len_small :
  forall value : nat,
    value < VAR_INT_U16_MARKER -> encoded_len value = 1.
Proof.
  intros value H.
  unfold encoded_len in *; unfold VAR_INT_U16_MARKER in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - reflexivity.
  - exfalso. lia.
Qed.

Lemma encoded_len_u16 :
  forall value : nat,
    VAR_INT_U16_MARKER <= value -> value <= MAX_U16 -> encoded_len value = 3.
Proof.
  intros value Hlo Hhi.
  unfold encoded_len in *; unfold VAR_INT_U16_MARKER in *; unfold MAX_U16 in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - exfalso. lia.
  - destruct (Nat.leb_spec value (256 * 256 - 1)) as [Hle | Hgt].
    + reflexivity.
    + exfalso. lia.
Qed.

Lemma encoded_len_u32 :
  forall value : nat,
    MAX_U16 < value -> value <= MAX_U32 -> encoded_len value = 5.
Proof.
  intros value Hlo Hhi.
  unfold encoded_len in *; unfold VAR_INT_U16_MARKER in *; unfold MAX_U16 in *; unfold MAX_U32 in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - exfalso. lia.
  - destruct (Nat.leb_spec value (256 * 256 - 1)) as [Hle | Hgt].
    + exfalso. lia.
    + destruct (Nat.leb_spec value (256 * 256 * 256 * 256 - 1)) as [Hle2 | Hgt2].
      * reflexivity.
      * exfalso. lia.
Qed.

Lemma encoded_len_u64 :
  forall value : nat,
    MAX_U32 < value -> encoded_len value = 9.
Proof.
  intros value H.
  unfold encoded_len in *; unfold VAR_INT_U16_MARKER in *; unfold MAX_U16 in *; unfold MAX_U32 in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - exfalso. lia.
  - destruct (Nat.leb_spec value (256 * 256 - 1)) as [Hle | Hgt].
    + exfalso. lia.
    + destruct (Nat.leb_spec value (256 * 256 * 256 * 256 - 1)) as [Hle2 | Hgt2].
      * exfalso. lia.
      * reflexivity.
Qed.

(** ---- Encoding preserves total output length for the small case ---- *)
Lemma write_small_length :
  forall value, value < VAR_INT_U16_MARKER -> length (write_var_int value) = 1.
Proof.
  intros value H.
  unfold write_var_int in *; unfold VAR_INT_U16_MARKER in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - reflexivity.
  - exfalso. lia.
Qed.

(** ---- Small-value encode/decode round-trip ---- *)
Lemma encode_small_then_decode :
  forall value, value < VAR_INT_U16_MARKER ->
  read_var_int_prefix (write_var_int value) = Some (value, 1).
Proof.
  intros value H.
  unfold write_var_int in *; unfold VAR_INT_U16_MARKER in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - unfold read_var_int_prefix, VAR_INT_U16_MARKER, VAR_INT_U32_MARKER.
    apply Nat.ltb_lt in Hlt.
    rewrite Hlt.
    reflexivity.
  - exfalso. lia.
Qed.

(** ---- The encoded prefix carries the value in the small case ---- *)
Lemma small_encode_prefix :
  forall value, value < VAR_INT_U16_MARKER ->
  hd 0 (write_var_int value) = value.
Proof.
  intros value H.
  unfold write_var_int in *; unfold VAR_INT_U16_MARKER in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - reflexivity.
  - exfalso. lia.
Qed.

(** ---- Output single byte is within [0,255] in the small case ---- *)
Lemma small_encode_byte_range :
  forall value b, value < VAR_INT_U16_MARKER -> In b (write_var_int value) ->
  b < 256.
Proof.
  intros value b H Hin.
  unfold write_var_int in *; unfold VAR_INT_U16_MARKER in *.
  destruct (Nat.ltb_spec value 253) as [Hlt | Hge].
  - simpl in Hin. destruct Hin as [Eq | Hin].
    + subst. lia.
    + inversion Hin.
  - exfalso. lia.
Qed.

End VarIntEncoding.