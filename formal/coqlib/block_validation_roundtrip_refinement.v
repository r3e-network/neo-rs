(**
  Block Validation Round-Trip — Coq 8.18 honest rebuild.

  The old file proved its "lemmas" by `apply <Axiom>` (assumed the very
  conclusions it was supposed to prove), used malformed syntax (`Definition
  ... and reduce_pairwise`), non-existent projections (proj1..proj8), and
  non-existent imports (OptionEq, Utf8).  It proved nothing machine-checkable.

  This honest rebuild keeps only the content that is genuinely provable,
  without axioms or Admitted:

    - BlockHeader carries an unsigned part and a separate witness list.
    - The header hash is computed over the unsigned part only, so attaching a
      different witness list leaves the hash unchanged (witness independence).
    - The Merkle root computed from a transaction list is deterministic:
      equal transaction lists produce equal roots (and single/two-element
      roots are computed exactly as in the Rust compute_root).
    - Everything is closed under an abstract `sha256` Section parameter that is
      a *given function*, not a theorem we claim to verify; no collision
      resistance or refinement of the real SHA-256 is asserted.

  Boundaries (no overclaim):
    - `deserialize (serialize b) = Some b` is NOT claimed here.  A true wire
      round-trip requires the implemented codec and size conservation proved in
      serialization_roundtrip_refinement; the old axiom-based "round-trip" was
      unsound and is dropped.
    - No claim that the full Rust Block structure or its hash is refined.
*)
From Coq Require Import List Arith Lia.
Import ListNotations.

Section BlockValidation.

(* Abstract hash primitive: a *given* function, not a verified theorem.
   (Explicit Section parameter; the caller must pick a concrete hash.  No
   collision resistance or Rust refinement is claimed.) *)
Variable sha256 : list nat -> list nat.

(* --- Merkle root (reflects neo-crypto merkle_tree.rs::compute_root) --- *)

Definition hash_pair (l r : list nat) : list nat :=
  sha256 (l ++ r).

(* One level of pairing: neighbours are paired; an odd tail is paired with
   itself (matching the Rust implementation).  Each node is a hash (list nat). *)
Fixpoint merkle_level (nodes : list (list nat)) : list (list nat) :=
  match nodes with
  | [] => []
  | [x] => [hash_pair x x]
  | x :: y :: rest => hash_pair x y :: merkle_level rest
  end.

(* Compute the merkle root with explicit fuel so the recursion is structural.
   Each level halves the node count (self-pairing keeps a single node), so
   fueling by `length nodes` is a safe upper bound on the number of levels. *)
Fixpoint compute_root_fuel (fuel : nat) (nodes : list (list nat)) : list (list nat) :=
  match fuel with
  | 0 => nodes
  | S f =>
      match nodes with
      | [] => []
      | [x] => [x]
      | _ => compute_root_fuel f (merkle_level nodes)
      end
  end.

Definition compute_root (nodes : list (list nat)) : list (list nat) :=
  compute_root_fuel (length nodes) nodes.

Lemma compute_root_deterministic :
  forall txs1 txs2 : list (list nat), txs1 = txs2 -> compute_root txs1 = compute_root txs2.
Proof.
  intros txs1 txs2 Heq. subst. reflexivity.
Qed.

(* Deterministic merkle root derived from a transaction list. *)
Lemma merkle_root_deterministic :
  forall txns1 txns2 : list (list nat),
    txns1 = txns2 -> compute_root txns1 = compute_root txns2.
Proof.
  intros txns1 txns2 Heq. apply compute_root_deterministic. exact Heq.
Qed.

Example merkle_empty : compute_root [] = [].
Proof. reflexivity. Qed.
Example merkle_single : forall x, compute_root [x] = [x].
Proof. intros x. reflexivity. Qed.
Example merkle_two :
  forall a b, compute_root [a; b] = [hash_pair a b].
Proof.
  intros a b. cbn [compute_root compute_root_fuel merkle_level length]. reflexivity.
Qed.

(* --- Header hash and witness independence --- *)

(* Header: unsigned fields (version, index) plus a separate witness list. *)
Record BlockHeader := make_header {
  h_version   : nat;
  h_index     : nat;
  h_witnesses : list nat
}.

(* Header hash covers ONLY the unsigned fields; witnesses are excluded. *)
Definition header_hash (h : BlockHeader) : list nat :=
  sha256 (h_version h :: h_index h :: nil).

(* Attach a fresh witness list; unsigned fields are preserved. *)
Definition with_witnesses (h : BlockHeader) (ws : list nat) : BlockHeader :=
  make_header (h_version h) (h_index h) ws.

(* Security-critical: changing witnesses does NOT change the header hash. *)
Lemma witness_independence :
  forall (h : BlockHeader) (ws : list nat),
    header_hash h = header_hash (with_witnesses h ws).
Proof.
  intros h ws. unfold header_hash, with_witnesses. simpl. reflexivity.
Qed.

(* The unsigned fields are unchanged when a witness list is attached, so the
   hash image is untouched for any two witness lists. *)
Lemma witness_change_irrelevant :
  forall (h : BlockHeader) (ws1 ws2 : list nat),
    header_hash (with_witnesses h ws1) = header_hash (with_witnesses h ws2).
Proof.
  intros h ws1 ws2.
  unfold header_hash, with_witnesses. simpl. reflexivity.
Qed.

End BlockValidation.