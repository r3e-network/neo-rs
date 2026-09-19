(**
  Commit Message Refinement — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, NOT a mechanical refinement proof):
    neo-consensus/src/messages/commit.rs::CommitMessage struct (lines 8-18)
      Fields: block_index: u32, view_number: u8, validator_index: u8, signature: Vec<u8>
    validate() checks signature length MUST be exactly 64 bytes (ECDSA r + s)
    serialize() returns the signature bytes directly as payload

  This is an ABSTRACT model of the commit message structure and its
  validation predicate.  It proves structural properties of the model:
    - serialization returns exactly the signature bytes,
    - valid commits have exactly 64-byte signatures,
    - validation iff safety (by definition).

  Boundaries:
    - No claim is made about ECDSA cryptographic correctness, secp256r1, or
      that a real Rust CommitMessage is refined here.
    - Signature byte range is modeled as nat <= 255; the actual cryptographic
      verification against a public key is out of scope.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

Section CommitModel.

(* Core commit message fields matching rust CommitMessage struct. *)
Record CommitMessage := make_commit {
  block_index   : nat;    (* u32 modeled as nat *)
  view_number   : nat;    (* u8 modeled as nat *)
  validator_idx : nat;    (* u8 modeled as nat *)
  signature     : list nat (* raw ECDSA signature bytes *)
}.

(* ECDSA signature size: 32-byte r + 32-byte s = 64 total. *)
Definition SIGNATURE_LENGTH : nat := 64.

(* Valid byte value range check for signature elements. *)
Definition is_valid_byte (n : nat) : Prop :=
  n <= 255.

(* All bytes in a signature must be valid (0-255). *)
Fixpoint all_bytes_valid (bs : list nat) : Prop :=
  match bs with
  | [] => True
  | b :: rest => is_valid_byte b /\ all_bytes_valid rest
  end.

(* A commit is VALID iff its signature is exactly 64 bytes and every byte is
   within [0,255].  Cryptographic verification is modeled as a structural
   contract (see module header). *)
Definition validate_commit (c : CommitMessage) : Prop :=
  length c.(signature) = SIGNATURE_LENGTH /\
  all_bytes_valid c.(signature).

(* Safety predicate: a validated commit can safely be processed. *)
Definition safe_to_process (c : CommitMessage) : Prop :=
  validate_commit c.

(* Serialization returns only the signature bytes (as per rust serialize()). *)
Definition serialize_commit (c : CommitMessage) : list nat :=
  c.(signature).

Theorem serialization_returns_signature_only :
  forall (c : CommitMessage),
    serialize_commit c = c.(signature).
Proof.
  intros c. unfold serialize_commit. reflexivity.
Qed.

Theorem serialize_length_for_valid_commit :
  forall (c : CommitMessage),
    validate_commit c ->
    length (serialize_commit c) = SIGNATURE_LENGTH.
Proof.
  intros c Hvalid.
  unfold serialize_commit in *.
  destruct Hvalid as [Hlen _]. exact Hlen.
Qed.

Lemma signature_length_correct :
  forall (c : CommitMessage),
    validate_commit c ->
    length c.(signature) = SIGNATURE_LENGTH.
Proof.
  intros c Hvalid. destruct Hvalid as [Hlen _]. exact Hlen.
Qed.

Lemma all_bytes_valid_bounded :
  forall (c : CommitMessage),
    validate_commit c -> all_bytes_valid c.(signature).
Proof.
  intros c Hvalid. destruct Hvalid as [_ Hall]. exact Hall.
Qed.

Lemma validation_preserves_safety :
  forall (c : CommitMessage),
    validate_commit c -> safe_to_process c.
Proof.
  intros c Hvalid. unfold safe_to_process. exact Hvalid.
Qed.

Lemma safety_requires_validation :
  forall (c : CommitMessage),
    safe_to_process c -> validate_commit c.
Proof.
  intros c Hsafe. unfold safe_to_process in Hsafe. exact Hsafe.
Qed.

Theorem validation_iff_safety :
  forall (c : CommitMessage),
    validate_commit c <-> safe_to_process c.
Proof.
  split; [apply validation_preserves_safety | apply safety_requires_validation].
Qed.

(* Any two valid commits have the same (constant) signature length. *)
Lemma valid_commits_have_constant_sig_len :
  forall c1 c2 : CommitMessage,
    validate_commit c1 ->
    validate_commit c2 ->
    length c1.(signature) = length c2.(signature).
Proof.
  intros c1 c2 H1 H2.
  rewrite (signature_length_correct c1 H1).
  rewrite (signature_length_correct c2 H2).
  reflexivity.
Qed.

(* Helper: a fully valid signature byte list (all values 42). *)
Lemma repeat_valid_bytes : forall (v n : nat),
  v <= 255 -> all_bytes_valid (repeat v n).
Proof.
  intros v n Hv. induction n; simpl; [auto | split; [exact Hv | exact IHn]].
Qed.

End CommitModel.

(** Concrete examples. *)
Section Examples.

Definition valid_commit_example :=
  make_commit 100 0 1 (repeat 0 64).

Theorem valid_commit_example_is_valid :
  validate_commit valid_commit_example.
Proof.
  unfold valid_commit_example, validate_commit.
  cbn [signature].
  split.
  - rewrite repeat_length. reflexivity.
  - apply repeat_valid_bytes. lia.
Qed.

Definition invalid_commit_short_sig :=
  make_commit 100 0 1 (repeat 0 32).

Theorem invalid_commit_short_sig_rejected :
  ~ validate_commit invalid_commit_short_sig.
Proof.
  unfold invalid_commit_short_sig, validate_commit.
  cbn [signature].
  intros [Hlen _]. rewrite repeat_length in Hlen. discriminate.
Qed.

Definition invalid_commit_long_sig :=
  make_commit 100 0 1 (repeat 0 128).

Theorem invalid_commit_long_sig_rejected :
  ~ validate_commit invalid_commit_long_sig.
Proof.
  unfold invalid_commit_long_sig, validate_commit.
  cbn [signature].
  intros [Hlen _]. rewrite repeat_length in Hlen. discriminate.
Qed.

Theorem empty_signature_invalid :
  forall c : CommitMessage,
    c.(signature) = [] -> ~ validate_commit c.
Proof.
  intros c Hempty Hvalid.
  destruct Hvalid as [Hlen _].
  rewrite Hempty in Hlen. simpl in Hlen. discriminate.
Qed.

End Examples.
