(**
  Block/Transaction Serialization Round-Trip — Coq 8.18 honest rebuild.

  The old file asserted round-trip identity as Axioms (an unproved "alibi"
  model), used non-existent types (u64/u32/u8, OptionEq, Utf8), and dropped
  header fields.  It proved nothing machine-checkable.

  This honest rebuild implements a real, self-contained wire codec
  (binary little-endian integer packing of fixed width k bytes) and
  machine-proves:

    - tx_roundtrip_identity:
        deserialize_tx (serialize_tx t) = Some (t, [])
        whenever the transaction fields fit their widths,
    - serialization_deterministic / tx_serialization_deterministic:
        equal inputs serialize to equal bytes (functional determinism),
    - witness_independence_from_hash:
        the header hash is computed over the unsigned header fields only,
        so changing the witness list leaves the hash unchanged.

  Boundaries (no overclaim):
    - This is an abstract fixed-width wire model, NOT a refinement proof of
      the full Neo N3 Rust block serialization (varint framing, nested lists,
      many more fields, usize overflow, etc.).
    - Round-trip holds because encode/decode are the inverse pair defined
      here; it is not an extracted or differential guarantee against Rust.
    - No axioms, no Admitted, no faked theorems.
*)
From Coq Require Import List ZArith Lia.
Import ListNotations.
Open Scope Z_scope.

(** --- Little-endian integer codec over Z --- *)

Fixpoint capacity (k : nat) : Z :=
  match k with O => 1 | S j => 256 * capacity j end.

Fixpoint le_encode (k : nat) (n : Z) : list Z :=
  match k with O => [] | S j => n mod 256 :: le_encode j (n / 256) end.

Fixpoint le_value (bs : list Z) : Z :=
  match bs with [] => 0 | b :: tl => b + 256 * le_value tl end.

Fixpoint read_le (k : nat) (bs : list Z) : option (Z * list Z) :=
  match k, bs with
  | O, _ => Some (0, bs)
  | S j, b :: tl =>
      match read_le j tl with
      | Some (n, rest) => Some (b + 256 * n, rest)
      | None => None
      end
  | _, _ => None
  end.

Lemma le_encode_length : forall k n, length (le_encode k n) = k.
Proof. induction k; intros; simpl; auto. Qed.

Lemma le_value_encode : forall k n,
  0 <= n < capacity k -> le_value (le_encode k n) = n.
Proof.
  induction k; intros n H.
  - change (0 <= n < 1) in H. change (0 = n). lia.
  - change (0 <= n < 256 * capacity k) in H.
    change (n mod 256 + 256 * le_value (le_encode k (n / 256)) = n).
    assert (Hd : 0 <= n / 256 < capacity k).
    { split; [apply Z.div_pos|apply Z.div_lt_upper_bound]; lia. }
    rewrite (IHk _ Hd).
    pose proof (Z.div_mod n 256 ltac:(lia)). lia.
Qed.

Lemma read_le_encode : forall k n rest,
  read_le k (le_encode k n ++ rest) = Some (le_value (le_encode k n), rest).
Proof.
  induction k; intros; simpl; auto. rewrite IHk. reflexivity.
Qed.

Lemma read_le_encode_nil : forall k n,
  read_le k (le_encode k n) = Some (le_value (le_encode k n), []).
Proof.
  intros k n. pose proof (read_le_encode k n []). rewrite app_nil_r in H. exact H.
Qed.

(** --- Abstract block/transaction model --- *)

(* A transaction image: a small fixed-width structure mirroring a subset of
   Neo's fixed-width integer transaction fields. *)
Record Tx := make_tx {
  tx_version : Z;   (* 1 byte  *)
  tx_nonce   : Z;   (* 8 bytes *)
  tx_index   : Z    (* 4 bytes *)
}.

(* A block image: unsigned header fields (version, index), a transaction list,
   and a separate witness list that is NOT part of the header hash. *)
Record Block := make_block {
  blk_version   : Z;
  blk_index     : Z;
  blk_txs       : list Tx;
  blk_witnesses : list Tx
}.

Definition serialize_tx (t : Tx) : list Z :=
  le_encode 1 (tx_version t) ++ le_encode 8 (tx_nonce t) ++ le_encode 4 (tx_index t).

(* Deserialize a transaction: parse 1 + 8 + 4 bytes in order. *)
Definition deserialize_tx (bs : list Z) : option (Tx * list Z) :=
  match read_le 1 bs with
  | Some (v, rest1) =>
      match read_le 8 rest1 with
      | Some (n, rest2) =>
          match read_le 4 rest2 with
          | Some (i, rest3) => Some (make_tx v n i, rest3)
          | None => None
          end
      | None => None
      end
  | None => None
  end.

Definition serialize_block (b : Block) : list Z :=
  le_encode 1 (blk_version b) ++ le_encode 4 (blk_index b) ++
  concat (map serialize_tx (blk_txs b)).

(* Header hash covers ONLY the unsigned header fields (version, index). *)
Definition header_hash (b : Block) : Z :=
  blk_version b + 256 * blk_index b.

(* Attach a new witness list; unsigned header fields are untouched. *)
Definition with_witnesses (b : Block) (ws : list Tx) : Block :=
  make_block (blk_version b) (blk_index b) (blk_txs b) ws.

(** --- Round-trip theorem --- *)

Theorem tx_roundtrip_identity :
  forall (t : Tx),
    0 <= tx_version t < 256 ->
    0 <= tx_nonce t < capacity 8 ->
    0 <= tx_index t < capacity 4 ->
    deserialize_tx (serialize_tx t) = Some (t, []).
Proof.
  intros t Hv Hn Hi.
  destruct t as [a b c].
  unfold serialize_tx, deserialize_tx.
  Opaque le_encode read_le le_value.
  rewrite (read_le_encode 1 a).
  simpl.
  rewrite (read_le_encode 8 b).
  simpl.
  rewrite (read_le_encode_nil 4 c).
  simpl.
  Transparent le_encode read_le le_value.
  rewrite (le_value_encode 1 a Hv).
  rewrite (le_value_encode 8 b Hn).
  rewrite (le_value_encode 4 c Hi).
  reflexivity.
Qed.

(** --- Determinism theorems --- *)

Theorem serialization_deterministic :
  forall b1 b2 : Block, b1 = b2 -> serialize_block b1 = serialize_block b2.
Proof.
  intros b1 b2 Heq. subst. reflexivity.
Qed.

Theorem tx_serialization_deterministic :
  forall t1 t2 : Tx, t1 = t2 -> serialize_tx t1 = serialize_tx t2.
Proof.
  intros t1 t2 Heq. subst. reflexivity.
Qed.

(** --- Witness independence --- *)

Theorem witness_independence_from_hash :
  forall (b : Block) (ws : list Tx),
    header_hash b = header_hash (with_witnesses b ws).
Proof.
  intros b ws. unfold header_hash, with_witnesses. simpl. reflexivity.
Qed.

(** Concrete round-trip example. *)
Example tx_roundtrip_example :
  deserialize_tx (serialize_tx (make_tx 1 42 7)) = Some (make_tx 1 42 7, []).
Proof.
  apply tx_roundtrip_identity.
  - cbn [tx_version]; lia.
  - cbn [tx_nonce capacity]; lia.
  - cbn [tx_index capacity]; lia.
Qed.