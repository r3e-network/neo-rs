From Coq Require Import List Arith Lia Bool.
Import ListNotations.

(** Multi-signature depth model aligned with neo-core/src/witness.rs
    `verify_multi_signature` (Neo N3).  ECDSA and the byte-lexicographic sort are
    explicit abstract Section parameters; no claim of refinement against the
    Secp256r1 crypto or Rust's `Vec<u8>::sort`.  The 40-byte N3 redeem-script /
    account-hash binding is modeled structurally in
    multisig_validation_refinement.v.

    Honest corrections vs the legacy model:
      * fewer-than-m signatures is a REAL guard (`signatures.len() != m` ->
        reject), not a vacuous one;
      * Rust does NOT reject duplicate public keys in the multi-sig path (they
        are sorted); the old "duplicate rejection" claim is dropped;
      * "one invalid signature never blocks valid ones" is FALSE under the
        greedy matching loop (a bad signature can consume keys and trigger the
        early `m - sig_index > n - key_index` rejection); only the weaker truth
        below is claimed: a valid signature with an available verifying key is
        counted.
*)

Section MultiSigDepth.

Definition bytes := list nat.
Definition public_key := bytes.
Definition signature := bytes.
Definition message := bytes.

(** Abstract Secp256r1 prehash verification oracle (mirrors Secp256r1Crypto::verify_prehash). *)
Variable ecdsa_verify : message -> signature -> public_key -> bool.

(** Abstract lexicographic sort over public keys (mirrors Vec<u8>::sort). *)
Variable sort_keys : list public_key -> list public_key.
Hypothesis sort_keys_length : forall ks, length (sort_keys ks) = length ks.

Definition max_keys : nat := 1024.

(** Valid multi-sig parameters: 1 <= m <= n <= 1024. *)
Definition valid_multisig_params (m n : nat) : Prop :=
  1 <= m /\ m <= n /\ n <= max_keys.

Definition params_valid_bool (m n : nat) : bool :=
  Nat.leb 1 m && Nat.leb m n && Nat.leb n max_keys.

(** THEOREM 1: the boolean guard agrees with the Prop predicate. *)
Lemma params_valid_bool_iff :
  forall m n, params_valid_bool m n = true <-> valid_multisig_params m n.
Proof.
  intros m n.
  unfold params_valid_bool, valid_multisig_params.
  rewrite Bool.andb_true_iff, Bool.andb_true_iff.
  rewrite !Nat.leb_le.
  tauto.
Qed.

(** Each submitted signature must be a 64-byte Secp256r1 r||s pair. *)
Definition sig_size_ok (s : signature) : Prop := length s = 64.

Fixpoint all_sig_sizes_ok (sigs : list signature) : bool :=
  match sigs with
  | [] => true
  | s :: ss => Nat.eqb (length s) 64 && all_sig_sizes_ok ss
  end.

(** Greedy matching of signatures to sorted keys (witness.rs loop): for the
    current signature, scan forward for the first key it verifies against;
    that key (and everything before it) is consumed. *)
Fixpoint try_verify_one (msg : message) (s : signature)
                       (keys : list public_key) : option (list public_key) :=
  match keys with
  | [] => None
  | k :: ks => if ecdsa_verify msg s k then Some ks else try_verify_one msg s ks
  end.

Fixpoint count_verified (msg : message) (sigs : list signature)
                        (keys : list public_key) : nat :=
  match sigs with
  | [] => 0
  | s :: ss =>
      match try_verify_one msg s keys with
      | None => 0
      | Some remaining => S (count_verified msg ss remaining)
      end
  end.

(** Full multi-sig verification mirroring witness.rs: guards then greedy match. *)
Definition verify_multi_signature (msg : message) (m : nat)
                                  (keys : list public_key) (sigs : list signature) : bool :=
  params_valid_bool m (length keys) &&
  Nat.eqb (length sigs) m &&
  all_sig_sizes_ok sigs &&
  Nat.eqb (count_verified msg sigs (sort_keys keys)) m.

(** THEOREM 2: fewer than m signatures is always rejected (real guard,
    non-vacuous). *)
Lemma under_signature_rejected :
  forall msg m keys sigs,
    length sigs < m -> verify_multi_signature msg m keys sigs = false.
Proof.
  intros msg m keys sigs Hless.
  unfold verify_multi_signature.
  assert (Hne : length sigs <> m) by lia.
  rewrite (proj2 (Nat.eqb_neq (length sigs) m) Hne).
  rewrite Bool.andb_false_r. simpl. reflexivity.
Qed.

(** THEOREM 3: a threshold larger than the key set is rejected
    (`required_signatures > public_keys.len()` -> false). *)
Lemma over_threshold_rejected :
  forall msg m keys sigs,
    m > length keys -> verify_multi_signature msg m keys sigs = false.
Proof.
  intros msg m keys sigs Hgt.
  unfold verify_multi_signature.
  assert (Hmgt : length keys < m) by lia.
  unfold params_valid_bool.
  destruct (Nat.leb m (length keys)) eqn:E; simpl.
  - apply Nat.leb_le in E. lia.
  - rewrite Bool.andb_false_r. simpl. reflexivity.
Qed.

(** THEOREM 4: zero threshold is rejected (`required_signatures == 0` -> false). *)
Lemma zero_threshold_rejected :
  forall msg keys sigs, verify_multi_signature msg 0 keys sigs = false.
Proof.
  intros. unfold verify_multi_signature, params_valid_bool. simpl. reflexivity.
Qed.

(** THEOREM 5: the sort used internally preserves key count (deterministic basis
    for the loop bound). *)
Lemma sort_preserves_length :
  forall keys, length (sort_keys keys) = length keys.
Proof. intros. apply sort_keys_length. Qed.

(** THEOREM 6: the number of verified signatures never exceeds the number
    submitted (loop progress is bounded by the signature list). *)
Lemma count_verified_le_length_sigs :
  forall msg sigs keys, count_verified msg sigs keys <= length sigs.
Proof.
  intros msg sigs. induction sigs as [|s ss IH]; intros keys; simpl; auto.
  destruct (try_verify_one msg s keys) as [remaining|]; simpl.
  - apply le_n_S. exact (IH remaining).
  - lia.
Qed.

(** THEOREM 7 (partial-failure truth, honest): a valid signature whose key is
    still available is always counted. *)
Lemma valid_sig_is_counted :
  forall msg (s : signature) (k : public_key) (keys : list public_key),
    ecdsa_verify msg s k = true ->
    count_verified msg (s :: nil) (k :: keys) = 1.
Proof.
  intros msg s k keys Hv.
  simpl. rewrite Hv. reflexivity.
Qed.

End MultiSigDepth.
