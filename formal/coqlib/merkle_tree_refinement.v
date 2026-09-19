Require Import Arith Lia List.
Import ListNotations.

(** Merkle root computation — honest model matching neo-crypto/src/merkle_tree.rs
    `compute_root`: reduce a hash list level by level; at each level pair
    adjacent hashes, and when the count is odd duplicate the final hash (hash it
    with itself).  SHA-256 is an explicit parameter (abstract); no claim of
    collision resistance or of matching the concrete Rust/C# hashing.

    `hash_pair left right := sha256 (left ++ right)` matches the Rust
    `hash_pair_internal` over the 64-byte concatenation.
*)

Section MerkleTree.

Definition bytes_ := list nat.
Variable sha256 : bytes_ -> bytes_.

Definition hash_pair (left right : bytes_) : bytes_ :=
  sha256 (left ++ right).

(** One reduction level: pair adjacent, duplicate a lone trailing element. *)
Fixpoint merkle_level (xs : list bytes_) : list bytes_ :=
  match xs with
  | [] => []
  | [x] => [hash_pair x x]
  | x :: y :: rest => hash_pair x y :: merkle_level rest
  end.

(** Fueled, structurally terminating root computation. *)
Fixpoint merkle_with_fuel (fuel : nat) (xs : list bytes_) : option bytes_ :=
  match xs with
  | [] => None
  | [x] => Some x
  | _ :: _ =>
      match fuel with
      | 0 => None
      | S fuel' => merkle_with_fuel fuel' (merkle_level xs)
      end
  end.

Definition compute_root (xs : list bytes_) : option bytes_ :=
  merkle_with_fuel (S (length xs)) xs.

(** Empty input -> None. *)
Lemma compute_root_empty : compute_root [] = None.
Proof. reflexivity. Qed.

(** Single element is its own root. *)
Lemma compute_root_single : forall h,
  compute_root [h] = Some h.
Proof. intros h. reflexivity. Qed.

(** Two elements reduce to their hash pair. *)
Lemma compute_root_two : forall h1 h2,
  compute_root [h1; h2] = Some (hash_pair h1 h2).
Proof. intros h1 h2. reflexivity. Qed.

(** Three elements: pair(h1,h2) then pair that with (h3,h3). *)
Lemma compute_root_three :
  forall h1 h2 h3,
  compute_root [h1; h2; h3] =
    Some (hash_pair (hash_pair h1 h2) (hash_pair h3 h3)).
Proof.
  intros h1 h2 h3. reflexivity.
Qed.

(** The root, when produced, is unique (function total on the domain). *)
Lemma compute_root_unique : forall xs r1 r2,
  compute_root xs = Some r1 -> compute_root xs = Some r2 -> r1 = r2.
Proof. intros xs r1 r2 H1 H2. congruence. Qed.

(** Determinism: equal inputs give equal roots. *)
Lemma compute_root_deterministic : forall xs ys,
  xs = ys -> compute_root xs = compute_root ys.
Proof. intros xs ys H. subst. reflexivity. Qed.

End MerkleTree.