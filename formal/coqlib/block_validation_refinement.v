From Coq Require Import Arith Bool List Lia.
Import ListNotations.

(**
  Small, kernel-checked block-validation model.

  This file deliberately models pure structural properties only.  It does not
  claim a refinement of the Rust serializer or of SHA-256: both are explicit
  parameters.  Protocol-specific serialization and cryptographic correctness
  require a separate, byte-level refinement.
*)

Definition byte := nat.
Definition is_byte (b : byte) : Prop := b < 256.
Definition uint256 := list byte.

Record Header := {
  version : nat;
  previous_hash : uint256;
  merkle_root : uint256;
  timestamp : nat;
  nonce : nat;
  index : nat;
  primary_index : nat;
  next_consensus : uint256;
  witnesses : list (list byte)
}.

Record Transaction := {
  tx_hash : uint256;
  tx_version : nat;
  tx_nonce : nat;
  sender : uint256;
  receiver : uint256;
  amount : nat;
  witness : list byte
}.

Record Block := {
  block_header : Header;
  transactions : list Transaction
}.

Section BlockValidation.

Variable max_block_size max_transactions_per_block : nat.
Variable min_timestamp_ms max_timestamp_drift_ms : nat.
Variable serialize_header_unsigned : Header -> list byte.
Variable sha256_hash : list byte -> uint256.
Variable hash_pair : uint256 -> uint256 -> uint256.

Definition compute_header_hash (h : Header) : uint256 :=
  sha256_hash (serialize_header_unsigned h).

Lemma header_hash_deterministic : forall h1 h2,
  h1 = h2 -> compute_header_hash h1 = compute_header_hash h2.
Proof. intros h1 h2 ->; reflexivity. Qed.

Definition unsigned_header_projection (h : Header) : Header :=
  {| version := version h;
     previous_hash := previous_hash h;
     merkle_root := merkle_root h;
     timestamp := timestamp h;
     nonce := nonce h;
     index := index h;
     primary_index := primary_index h;
     next_consensus := next_consensus h;
     witnesses := [] |}.

(** The serializer contract needed for witness independence is explicit. *)
Hypothesis serialize_unsigned_projection : forall h,
  serialize_header_unsigned h =
  serialize_header_unsigned (unsigned_header_projection h).

Lemma hash_independent_of_witnesses : forall h1 h2,
  unsigned_header_projection h1 = unsigned_header_projection h2 ->
  compute_header_hash h1 = compute_header_hash h2.
Proof.
  intros h1 h2 H.
  unfold compute_header_hash.
  rewrite (serialize_unsigned_projection h1).
  rewrite (serialize_unsigned_projection h2).
  rewrite H.
  reflexivity.
Qed.

(** One Merkle level.  A lone final leaf is duplicated, as in Neo's tree. *)
Fixpoint merkle_level (xs : list uint256) : list uint256 :=
  match xs with
  | [] => []
  | [x] => [hash_pair x x]
  | x :: y :: rest => hash_pair x y :: merkle_level rest
  end.

(** Fuel makes the recursive abstract model structurally terminating. *)
Fixpoint merkle_with_fuel (fuel : nat) (xs : list uint256) : option uint256 :=
  match xs with
  | [] => None
  | [x] => Some x
  | _ :: _ =>
      match fuel with
      | 0 => None
      | S fuel' => merkle_with_fuel fuel' (merkle_level xs)
      end
  end.

Definition compute_merkle_root (xs : list uint256) : option uint256 :=
  merkle_with_fuel (S (length xs)) xs.

Lemma compute_merkle_root_empty : compute_merkle_root [] = None.
Proof. reflexivity. Qed.

Lemma compute_merkle_root_single_element : forall h,
  compute_merkle_root [h] = Some h.
Proof. intros h; reflexivity. Qed.

Lemma compute_merkle_root_two_elements : forall h1 h2,
  compute_merkle_root [h1; h2] = Some (hash_pair h1 h2).
Proof. intros h1 h2; reflexivity. Qed.

Lemma merkle_level_pair : forall h1 h2,
  merkle_level [h1; h2] = [hash_pair h1 h2].
Proof. reflexivity. Qed.

Definition validate_block_version (h : Header) : bool :=
  Nat.eqb (version h) 0.

Lemma valid_version_is_zero : forall h,
  validate_block_version h = true -> version h = 0.
Proof.
  intros h H. unfold validate_block_version in H.
  now apply Nat.eqb_eq.
Qed.

Definition validate_primary_index (h : Header) (validators_count : nat) : bool :=
  Nat.ltb (primary_index h) validators_count.

Lemma valid_primary_index_bounds : forall h count,
  validate_primary_index h count = true -> primary_index h < count.
Proof.
  intros h count H. unfold validate_primary_index in H.
  now apply Nat.ltb_lt.
Qed.

Definition validate_timestamp_range (ts current_time : nat) : bool :=
  Nat.leb min_timestamp_ms ts &&
  Nat.leb ts current_time &&
  Nat.leb (current_time - ts) max_timestamp_drift_ms.

Lemma timestamp_lower_bound : forall ts current_time,
  validate_timestamp_range ts current_time = true -> min_timestamp_ms <= ts.
Proof.
  intros ts current_time H.
  unfold validate_timestamp_range in H.
  apply andb_true_iff in H as [H1 _].
  apply andb_true_iff in H1 as [H _].
  apply Nat.leb_le in H.
  exact H.
Qed.

Definition validate_transaction_count (xs : list Transaction) : bool :=
  Nat.leb (length xs) max_transactions_per_block.

Lemma valid_transaction_count_bound : forall xs,
  validate_transaction_count xs = true -> length xs <= max_transactions_per_block.
Proof.
  intros xs H. unfold validate_transaction_count in H.
  now apply Nat.leb_le.
Qed.

Definition get_varint_size (value : nat) : nat :=
  if value <? 253 then 1 else if value <? 65536 then 3 else 9.

Definition serialize_block_size (b : Block) : nat :=
  length (serialize_header_unsigned (block_header b)) +
  fold_left (fun acc w => acc + length w)
    (witnesses (block_header b)) 0 +
  fold_left (fun acc tx => acc + length (witness tx))
    (transactions b) 0 +
  get_varint_size (length (transactions b)).

Definition validate_block_size_constraint (b : Block) : bool :=
  Nat.leb (serialize_block_size b) max_block_size.

Lemma valid_size_constraint : forall b,
  validate_block_size_constraint b = true ->
  serialize_block_size b <= max_block_size.
Proof.
  intros b H. unfold validate_block_size_constraint in H.
  now apply Nat.leb_le.
Qed.

(** A coherent structural predicate includes the validator-count witness. *)
Definition valid_block_structure (validator_count : nat) (b : Block) : Prop :=
  validate_block_version (block_header b) = true /\
  validate_transaction_count (transactions b) = true /\
  validate_block_size_constraint b = true /\
  compute_merkle_root (map tx_hash (transactions b)) =
    Some (merkle_root (block_header b)) /\
  validator_count > 0 /\
  validate_primary_index (block_header b) validator_count = true.

Theorem valid_block_satisfies_constraints : forall validator_count b,
  validate_block_version (block_header b) = true ->
  validate_transaction_count (transactions b) = true ->
  validate_block_size_constraint b = true ->
  compute_merkle_root (map tx_hash (transactions b)) =
    Some (merkle_root (block_header b)) ->
  validator_count > 0 ->
  validate_primary_index (block_header b) validator_count = true ->
  valid_block_structure validator_count b.
Proof.
  intros validator_count b Hv Ht Hs Hm Hpositive Hprimary.
  repeat split; assumption.
Qed.

Lemma merkle_root_unique : forall xs r1 r2,
  compute_merkle_root xs = Some r1 ->
  compute_merkle_root xs = Some r2 -> r1 = r2.
Proof.
  intros xs r1 r2 H1 H2. congruence.
Qed.

End BlockValidation.
