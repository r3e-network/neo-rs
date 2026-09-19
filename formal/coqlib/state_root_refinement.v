From Coq Require Import List Arith Lia Bool.
From Coq Require Import Sorting.Permutation.
Import ListNotations.

(** Merkle Patricia Trie state-root model aligned with neo-crypto/src/mpt_trie/
    (node.rs, trie.rs).  SHA-256 and the abstract multi-leaf combination are
    explicit Section parameters; no claim of collision resistance or of a
    byte-exact match to the concrete Rust node serialization.

    Honest corrections vs the legacy model:
      * an EMPTY trie has NO root (`Trie::root_hash` returns `None`), so the old
        `empty_trie_root_constant : compute_mpt_root empty = Some (sha256 [])`
        is FALSE and is replaced by `root_hash_empty : root_hash [] = None`;
      * a single-leaf root is the hash of that serialized leaf
        (`leaf_hash v = sha256 ([LeafByte] ++ v)`), not the fabricated
        `hash_pair ... (sha256 [])`;
      * no collision-resistance claim is made (`root_resistant_to_leaf_modification`
        is dropped — it would require hash injectivity).
*)

Section MPTRoot.

Definition bytes := list nat.

(** Abstract SHA-256 on bytes. *)
Variable sha256 : bytes -> bytes.

(** Node-type byte for a leaf (abstract value in [0..255]). *)
Variable leaf_type_byte : nat.
Hypothesis leaf_type_byte_range : leaf_type_byte < 256.

(** Leaf serialization: node-type byte followed by the value.  In the Rust/C#
    implementation a leaf node serializes as its type byte then the value
    (the path is carried by the trie structure, not the leaf). *)
Definition serialize_leaf (value : bytes) : bytes :=
  leaf_type_byte :: value.

Definition leaf_hash (value : bytes) : bytes :=
  sha256 (serialize_leaf value).

(** Abstract combination of leaf hashes into a multi-leaf root. *)
Variable tree_hash : list bytes -> bytes.
Hypothesis tree_hash_permutation :
  forall l1 l2, Permutation l1 l2 -> tree_hash l1 = tree_hash l2.

(** A key-value state modeled as an association list (key, value). *)
Definition state := list (bytes * bytes).

Definition root_hash (st : state) : option bytes :=
  match st with
  | [] => None
  | [_] => Some (leaf_hash (snd (hd (nil, nil) st)))
  | _ => Some (tree_hash (map snd st))
  end.

(** THEOREM: an empty trie has no root (corrects the legacy false claim). *)
Lemma root_hash_empty : root_hash [] = None.
Proof. reflexivity. Qed.

(** THEOREM: a single-leaf state roots at the hash of that leaf. *)
Lemma root_hash_single :
  forall k v, root_hash [(k, v)] = Some (leaf_hash v).
Proof. intros k v. reflexivity. Qed.

(** THEOREM: equal states give equal roots (determinism). *)
Lemma root_hash_deterministic :
  forall st1 st2, st1 = st2 -> root_hash st1 = root_hash st2.
Proof. intros st1 st2 Heq. subst. reflexivity. Qed.

(** THEOREM: leaf hashing is deterministic. *)
Lemma leaf_hash_deterministic :
  forall v1 v2, v1 = v2 -> leaf_hash v1 = leaf_hash v2.
Proof. intros v1 v2 Heq. subst. reflexivity. Qed.

(** THEOREM: the root is independent of the order in which equal key/value
    entries are listed (matches the legacy intent, honestly). *)
Lemma root_independent_of_insertion_order :
  forall st1 st2, Permutation st1 st2 -> root_hash st1 = root_hash st2.
Proof.
  intros st1 st2 Hperm.
  destruct st1 as [|p1 st1'].
  - (* st1 empty *)
    apply Permutation_nil in Hperm. subst. reflexivity.
  - destruct st1' as [|p2 st1''].
    + (* st1 single *)
      apply Permutation_length_1_inv in Hperm. subst. reflexivity.
    + (* st1 has at least two entries *)
      simpl.
      destruct st2 as [|q1 st2'].
      * symmetry in Hperm. apply Permutation_nil in Hperm. inversion Hperm.
      * destruct st2' as [|q2 st2''].
        -- symmetry in Hperm.
           apply Permutation_length_1_inv in Hperm. inversion Hperm.
        -- simpl. f_equal.
           apply (tree_hash_permutation (map snd (p1 :: p2 :: st1''))
                                        (map snd (q1 :: q2 :: st2''))).
           apply (Permutation_map snd). exact Hperm.
Qed.

(** ---- End: root-hash semantics ---- *)

(** Concrete example: an empty state yields no root. *)
Example empty_state_no_root : root_hash [] = None.
Proof. reflexivity. Qed.

End MPTRoot.
