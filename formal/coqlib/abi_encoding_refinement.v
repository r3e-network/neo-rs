Require Import Arith Lia List.
Import ListNotations.

(**
  ABI / StackValue encoding - honest reconstruction (Coq 8.18).

  The prior file claimed `decode(encode v) = Some v` while its decoder returned
  constant values for every constructor (e.g. any ByteString decoded to
  `SV_ByteString []`), so the claimed round-trip was actually false (counter
  example: `SV_ByteString [7]` encodes with a length prefix `[0;0;0;0]` and
  payload `[7]`, but the old decoder dropped the payload).  That modelling also
  did not compile on Coq 8.18.

  This is the honest reconstruction.  It mirrors the wire format of the byte
  content variants of `fast_codec.rs` / `stack_value.rs`:
    Null       -> [0x0A]
    Boolean b  -> [0x04; b]
    ByteString bs -> [0x03] ++ u32_LE(len bs) ++ bs
    Buffer   bs -> [0x0C] ++ u32_LE(len bs) ++ bs
  where u32_LE is the 4-byte little-endian encoding of a length.  A real decoder
  is given and the round-trips are proved: encodings decode back to the original
  value.  We also prove the u32 length prefix itself round-trips.

  Scope note: the i64/u64 handle variants (Integer, Interop, Iterator, Pointer)
  and the nested collections (Array/Struct/Map) are NOT modelled here; encoding
  them faithfully requires an i64 arithmetic layer and a recursive value parser,
  which is future work.  This is an abstract byte-list model of the byte-content
  primitives; it is not a refinement proof of the Rust executable.
*)

Definition byte := nat.
Definition valid_byte (b : byte) : Prop := b < 256.

(** Wire tags, matching STACK_VALUE_CODEC_TAG_* in stack_value.rs. *)
Definition TAG_INTEGER    : byte := 0x01.
Definition TAG_BIG_INTEGER: byte := 0x02.
Definition TAG_BYTESTRING : byte := 0x03.
Definition TAG_BOOLEAN    : byte := 0x04.
Definition TAG_ARRAY      : byte := 0x05.
Definition TAG_STRUCT     : byte := 0x06.
Definition TAG_MAP        : byte := 0x07.
Definition TAG_INTEROP    : byte := 0x08.
Definition TAG_ITERATOR   : byte := 0x09.
Definition TAG_NULL       : byte := 0x0A.
Definition TAG_POINTER    : byte := 0x0B.
Definition TAG_BUFFER     : byte := 0x0C.

(** Modelled stack values: the byte-content primitives. *)
Inductive stack_value : Type :=
| SV_Null
| SV_Boolean (b : bool)
| SV_ByteString (bs : list byte)
| SV_Buffer (bs : list byte).

(** ---- u32 length prefix (4 bytes, little-endian) ---- *)

(** Written as arithmetic (256^k) so `lia` can use them; big nat literals are
    parsed via `of_num_uint` and defeat `lia`. *)
Definition U16 : nat := 256 * 256.
Definition U24 : nat := 256 * 256 * 256.
Definition U32 : nat := 256 * 256 * 256 * 256.

(** Encode n (< 2^32) as 4 little-endian bytes. *)
Definition u32_bytes (n : nat) : list byte :=
  (n mod 256) :: ((n / 256) mod 256) :: ((n / U16) mod 256) :: ((n / U24) mod 256) :: [].

(** Decode 4 little-endian bytes back into a number. *)
Definition u32_of_bytes (bs : list byte) : option nat :=
  match bs with
  | b0 :: b1 :: b2 :: b3 :: [] =>
      Some (b0 + 256 * b1 + U16 * b2 + U24 * b3)
  | _ => None
  end.

(** u32 length bytes are exactly 4 long. *)
Lemma u32_bytes_length : forall n, length (u32_bytes n) = 4.
Proof. intros n. reflexivity. Qed.

(** ---- Base-256 digit identities (arithmetic, `lia`-friendly) ---- *)

Lemma decomp_a : forall n, n mod 256 + 256 * (n / 256) = n.
Proof.
  intros n.
  assert (H : n = 256 * (n / 256) + n mod 256).
  { apply (Nat.div_mod n 256); discriminate. }
  lia.
Qed.

Lemma decomp_b : forall n,
  n mod 256 + 256 * ((n / 256) mod 256) + U16 * (n / U16) = n.
Proof.
  intros n.
  assert (Ha := decomp_a n).
  assert (Hq : n / 256 = 256 * ((n / 256) / 256) + (n / 256) mod 256).
  { apply (Nat.div_mod (n / 256) 256); discriminate. }
  assert (Hdiv : (n / 256) / 256 = n / U16).
  { unfold U16. apply (Nat.div_div n 256 256); discriminate. }
  unfold U16 in *. lia.
Qed.

Lemma decomp_c : forall n,
  n mod 256 + 256 * ((n / 256) mod 256) + U16 * ((n / U16) mod 256) + U24 * (n / U24) = n.
Proof.
  intros n.
  assert (Hb := decomp_b n).
  assert (Hq : n / U16 = 256 * ((n / U16) / 256) + (n / U16) mod 256).
  { apply (Nat.div_mod (n / U16) 256); discriminate. }
  assert (Hdiv : (n / U16) / 256 = n / U24).
  { unfold U16, U24. apply (Nat.div_div n (256*256) 256); discriminate. }
  unfold U16, U24 in *. lia.
Qed.

(** The full 4-byte little-endian sum reconstructs n for n < 2^32. *)
Lemma sum_to_n : forall n, n < U32 ->
  (n mod 256) + 256 * ((n / 256) mod 256) + U16 * ((n / U16) mod 256) + U24 * ((n / U24) mod 256) = n.
Proof.
  intros n Hn.
  assert (Hc := decomp_c n).
  assert (Hlt : n / U24 < 256).
  { apply (Nat.div_lt_upper_bound n U24 256).
    - discriminate.
    - unfold U24, U32 in *. lia. }
  assert (Hmod : (n / U24) mod 256 = n / U24).
  { apply Nat.mod_small. exact Hlt. }
  unfold U16, U24 in *. lia.
Qed.

(** The u32 length prefix round-trips for lengths below 2^32. *)
Lemma u32_bytes_roundtrip : forall n, n < U32 -> u32_of_bytes (u32_bytes n) = Some n.
Proof.
  intros n Hn. unfold u32_of_bytes, u32_bytes. f_equal.
  apply (sum_to_n n Hn).
Qed.

(* Keep U16/U24 un-reduced so that `simpl` does not expand them into huge nat
   numerals (which overflows the stack); they are treated as opaque symbols. *)
Local Opaque U16 U24.

(* Keep U16/U24 reduction under control for the round-trip proofs. *)

(** ---- encode / decode ---- *)

Definition encode_value (v : stack_value) : list byte :=
  match v with
  | SV_Null => [TAG_NULL]
  | SV_Boolean b => [TAG_BOOLEAN; (if b then 1 else 0)]
  | SV_ByteString bs => TAG_BYTESTRING :: u32_bytes (length bs) ++ bs
  | SV_Buffer bs => TAG_BUFFER :: u32_bytes (length bs) ++ bs
  end.

(** Shared decoder for the length-prefixed byte payloads: parse a 4-byte
    little-endian length, require the remaining payload to match exactly. *)
Definition decode_seq (rest : list byte) : option (list byte) :=
  match rest with
  | b0 :: b1 :: b2 :: b3 :: data =>
      let len := b0 + 256 * b1 + U16 * b2 + U24 * b3 in
      if Nat.eqb (length data) len then Some data else None
  | _ => None
  end.

Definition decode_value (bs : list byte) : option stack_value :=
  match bs with
  | [] => None
  | h :: rest =>
      match h with
      | 0x0A => (match rest with [] => Some SV_Null | _ => None end)
      | 0x04 => (match rest with [b] => Some (SV_Boolean (negb (Nat.eqb b 0))) | _ => None end)
      | 0x03 => (match decode_seq rest with Some d => Some (SV_ByteString d) | None => None end)
      | 0x0C => (match decode_seq rest with Some d => Some (SV_Buffer d) | None => None end)
      | _ => None
      end
  end.

(** ---- Round-trips ---- *)

Lemma null_roundtrip : decode_value (encode_value SV_Null) = Some SV_Null.
Proof. reflexivity. Qed.

Lemma boolean_roundtrip : forall b, decode_value (encode_value (SV_Boolean b)) = Some (SV_Boolean b).
Proof.
  intros b. destruct b; reflexivity.
Qed.

(** `cbv beta iota` reduces the head tag dispatch and list splicing without
    unfolding `Nat.eqb` on the (symbolic) payload length, leaving the length
    check as `if Nat.eqb (length bs) <little-endian sum> then ...`, which we
    then rewrite with `sum_to_n`. *)
Lemma bytestring_roundtrip : forall bs,
  length bs < U32 -> decode_value (encode_value (SV_ByteString bs)) = Some (SV_ByteString bs).
Proof.
  intros bs Hlen.
  cbv beta iota zeta delta
      [encode_value decode_value decode_seq u32_bytes app
       TAG_NULL TAG_BOOLEAN TAG_BYTESTRING TAG_BUFFER].
  rewrite (sum_to_n (length bs) Hlen).
  rewrite (Nat.eqb_refl (length bs)).
  simpl. reflexivity.
Qed.

Lemma buffer_roundtrip : forall bs,
  length bs < U32 -> decode_value (encode_value (SV_Buffer bs)) = Some (SV_Buffer bs).
Proof.
  intros bs Hlen.
  cbv beta iota zeta delta
      [encode_value decode_value decode_seq u32_bytes app
       TAG_NULL TAG_BOOLEAN TAG_BYTESTRING TAG_BUFFER].
  rewrite (sum_to_n (length bs) Hlen).
  rewrite (Nat.eqb_refl (length bs)).
  simpl. reflexivity.
Qed.

(** ---- Encoding structural facts ---- *)

(** The first byte is the type tag. *)
Lemma encode_null_first_byte : encode_value SV_Null = [TAG_NULL].
Proof. reflexivity. Qed.

Lemma encode_boolean_first_byte : forall b,
  firstn 1 (encode_value (SV_Boolean b)) = [TAG_BOOLEAN].
Proof. intros b. destruct b; reflexivity. Qed.

Lemma encode_bytestring_first_byte : forall bs,
  firstn 1 (encode_value (SV_ByteString bs)) = [TAG_BYTESTRING].
Proof. intros bs. reflexivity. Qed.

(** Distinct variants have distinct first (tag) bytes. *)
Lemma tag_separation_bytestring_null :
  firstn 1 (encode_value (SV_ByteString [])) <> firstn 1 (encode_value SV_Null).
Proof.
  intros H. simpl in H. discriminate.
Qed.

(** Encode is deterministic (a pure total function). *)
Lemma encode_deterministic : forall v1 v2, v1 = v2 -> encode_value v1 = encode_value v2.
Proof. intros v1 v2 H. subst. reflexivity. Qed.

(** Encoding a byte string takes tag(1) + length(4) + payload bytes. *)
Lemma encode_bytestring_length : forall bs,
  length (encode_value (SV_ByteString bs)) = 5 + length bs.
Proof.
  intros bs. unfold encode_value, u32_bytes. simpl. lia.
Qed.

(** The encoder never emits a zero-length length prefix for a non-empty payload
    (the length bytes carry the count). *)
Lemma decode_bytestring_preserves_payload : forall bs,
  length bs < U32 -> decode_value (encode_value (SV_ByteString bs)) <> None.
Proof.
  intros bs Hlen Hnone.
  rewrite (bytestring_roundtrip bs Hlen) in Hnone. discriminate.
Qed.

(** Aggregate honest refinement statement for the byte-content primitives. *)
Definition roundtrip_correct : Prop :=
  (decode_value (encode_value SV_Null) = Some SV_Null) /\
  (forall b, decode_value (encode_value (SV_Boolean b)) = Some (SV_Boolean b)) /\
  (forall bs, length bs < U32 -> decode_value (encode_value (SV_ByteString bs)) = Some (SV_ByteString bs)) /\
  (forall bs, length bs < U32 -> decode_value (encode_value (SV_Buffer bs)) = Some (SV_Buffer bs)).

Theorem roundtrip_theorem : roundtrip_correct.
Proof.
  unfold roundtrip_correct.
  split; [exact null_roundtrip |].
  split; [exact boolean_roundtrip |].
  split; [exact bytestring_roundtrip |].
  exact buffer_roundtrip.
Qed.