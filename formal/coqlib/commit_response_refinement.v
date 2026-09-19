(**
  Commit Response Validation Refinement — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, abstract; NOT a mechanical refinement):
    neo-consensus/src/context/mod.rs::add_commit (476-494), has_enough_commits (349-370);
    commit signatures are ECDSA over secp256r1 (64 bytes r||s);
    quorum M = n - f with network size n and faulty f.

  This is an ABSTRACT model of the commit-response structure, its signature
  validation, the dBFT quorum arithmetic, and the first-writer-wins property
  of the commit store.  Machine-checked:
    - valid commits have exactly 64-byte signatures (all bytes <= 255),
    - serialization returns the signature bytes,
    - honest-majority bound M = n - f > f when f*3 < n and the concrete Neo N3
      quorum sizes,
    - first-writer-wins: adding a commit for a (validator,view) pair that is not
      yet present yields exactly one entry for it (no double counting).

  Boundaries:
    - No cryptographic/ECDSA claim; signature byte structure is modeled only.
    - No claim that the Rust/c# commit handling is refined.
    - The store is modeled as a flat list of (validator,view,signature) triples.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

Section CommitResponseModel.

Record CommitResponse := make_commit_resp {
  validator_idx : nat;
  block_hash    : list nat;   (* UInt256 (32 bytes), modeled as a byte list *)
  view_number   : nat;
  signature     : list nat
}.

Definition SIGNATURE_LENGTH : nat := 64.
Definition BLOCK_HASH_LENGTH : nat := 32.

Definition is_valid_byte (n : nat) : Prop := n <= 255.

Fixpoint all_bytes_valid (bs : list nat) : Prop :=
  match bs with
  | [] => True
  | b :: rest => is_valid_byte b /\ all_bytes_valid rest
  end.

Lemma repeat_valid_bytes : forall v n,
  v <= 255 -> all_bytes_valid (repeat v n).
Proof.
  intros v n Hv. induction n; simpl; [auto | split; [exact Hv | exact IHn]].
Qed.

(* A commit response is VALID iff its signature is exactly 64 bytes with all
   bytes in [0,255] and its block hash is exactly 32 bytes (UInt256). *)
Definition validate_commit_response (cr : CommitResponse) : Prop :=
  length cr.(signature) = SIGNATURE_LENGTH /\
  all_bytes_valid cr.(signature) /\
  length cr.(block_hash) = BLOCK_HASH_LENGTH.

Definition safe_to_process_commit (cr : CommitResponse) : Prop :=
  validate_commit_response cr.

(* Serialization returns the signature bytes (per rust serialize). *)
Definition serialize_commit (cr : CommitResponse) : list nat :=
  cr.(signature).

Theorem serialize_returns_signature :
  forall cr, serialize_commit cr = cr.(signature).
Proof. intros cr. unfold serialize_commit. reflexivity. Qed.

Theorem signature_length_for_valid :
  forall cr, validate_commit_response cr ->
    length (serialize_commit cr) = SIGNATURE_LENGTH.
Proof.
  intros cr Hvalid. unfold serialize_commit.
  destruct Hvalid as [Hlen _]. exact Hlen.
Qed.

Theorem validation_iff_safety :
  forall cr, validate_commit_response cr <-> safe_to_process_commit cr.
Proof.
  intros cr. split.
  - intros H. unfold safe_to_process_commit. exact H.
  - intros H. unfold safe_to_process_commit in H. exact H.
Qed.

End CommitResponseModel.

(** Commit store: first-writer-wins with no double counting. *)
Section CommitStore.

(* Entry matches a (validator, view) pair. *)
Definition entry_matches (e : nat * nat * list nat) (idx view : nat) : bool :=
  match e with
  | (v, vn, _) => (v =? idx) && (vn =? view)
  end.

Fixpoint count_entries (idx view : nat) (l : list (nat * nat * list nat)) : nat :=
  match l with
  | [] => 0
  | e :: rest => if entry_matches e idx view then S (count_entries idx view rest)
                 else count_entries idx view rest
  end.

(* Add a commit; if the (idx,view) pair is already present, keep the first. *)
Fixpoint add_commit_if_not_exists (idx view : nat) (sig : list nat)
                                 (l : list (nat * nat * list nat)) : list (nat * nat * list nat) :=
  match l with
  | [] => [(idx, view, sig)]
  | e :: rest =>
      if entry_matches e idx view then l
      else e :: add_commit_if_not_exists idx view sig rest
  end.

(* First-writer-wins: if the (idx,view) pair is absent, after adding there is
   exactly one entry for it (the added one).  No double counting occurs. *)
Theorem add_increments_count :
  forall idx view sig l,
    count_entries idx view l = 0 ->
    count_entries idx view (add_commit_if_not_exists idx view sig l) = 1.
Proof.
  intros idx view sig l.
  induction l as [| e rest IH]; intros H0.
  - simpl.
    unfold entry_matches.
    rewrite !Nat.eqb_refl.
    simpl. reflexivity.
  - simpl in H0.
    destruct (entry_matches e idx view) eqn:Hm.
    + simpl in H0. discriminate.
    + simpl. rewrite Hm. simpl. rewrite Hm. apply IH. exact H0.
Qed.

(* If the pair is already present, adding keeps it present exactly once. *)
Theorem add_keeps_present :
  forall idx view sig l,
    count_entries idx view l = 1 ->
    count_entries idx view (add_commit_if_not_exists idx view sig l) = 1.
Proof.
  intros idx view sig l.
  induction l as [| e rest IH]; intros H1.
  - simpl in H1. discriminate.
  - simpl in H1.
    destruct (entry_matches e idx view) eqn:Hm.
    + simpl. rewrite Hm. simpl. rewrite Hm. exact H1.
    + simpl. rewrite Hm. simpl. rewrite Hm. apply IH. exact H1.
Qed.

End CommitStore.

(** dBFT quorum arithmetic: M = n - f, honest majority. *)
Section Quorum.

Definition compute_f (n : nat) : nat := Nat.div (Nat.sub n 1) 3.
Definition quorum (n f : nat) : nat := n - f.

Theorem quorum_formula : forall n f, quorum n f = n - f.
Proof. intros n f. unfold quorum. reflexivity. Qed.

Theorem honest_majority :
  forall n f : nat, f * 3 < n -> quorum n f > f.
Proof.
  intros n f H. unfold quorum. lia.
Qed.

(* Neo N3 concrete sizes: n=4 -> M=3, n=7 -> 5, n=10 -> 7, n=13 -> 9. *)
Theorem M_formula_verified_NeoN3 :
  quorum 4 (compute_f 4) = 3 /\ quorum 7 (compute_f 7) = 5 /\
  quorum 10 (compute_f 10) = 7 /\ quorum 13 (compute_f 13) = 9.
Proof.
  repeat split; vm_compute; reflexivity.
Qed.

(* Count commits that carry a given view number. *)
Fixpoint count_view_votes (v : nat) (l : list (nat * nat * list nat)) : nat :=
  match l with
  | [] => 0
  | (_, vn, _) :: rest => if vn =? v then S (count_view_votes v rest)
                          else count_view_votes v rest
  end.

Definition has_enough_commits (n v : nat) (l : list (nat * nat * list nat)) : Prop :=
  count_view_votes v l >= quorum n (compute_f n).

(* A numeric witness: with n = 4 (M = 3), three commits at view 0 suffice. *)
Theorem finality_example_n4 :
  has_enough_commits 4 0
    [(0,0,repeat 0 64); (1,0,repeat 1 64); (2,0,repeat 2 64)].
Proof.
  unfold has_enough_commits, quorum, compute_f.
  cbn. lia.
Qed.

End Quorum.

(** Concrete example: a valid commit response + rejected short signature. *)
Section Examples.

Definition valid_commit_example :=
  make_commit_resp 1 (repeat 16 32) 0 (repeat 42 64).

Theorem valid_commit_is_valid : validate_commit_response valid_commit_example.
Proof.
  unfold valid_commit_example, validate_commit_response, BLOCK_HASH_LENGTH, SIGNATURE_LENGTH.
  cbn [signature block_hash].
  split.
  - rewrite repeat_length. reflexivity.
  - split.
    + apply repeat_valid_bytes.
      apply Nat.leb_le. vm_compute. reflexivity.
    + rewrite repeat_length. reflexivity.
Qed.

Definition invalid_commit_short_sig :=
  make_commit_resp 1 (repeat 16 32) 0 (repeat 42 32).

Theorem invalid_commit_rejected : ~ validate_commit_response invalid_commit_short_sig.
Proof.
  unfold invalid_commit_short_sig, validate_commit_response.
  cbn [signature].
  intros [Hlen _]. rewrite repeat_length in Hlen. discriminate.
Qed.

End Examples.