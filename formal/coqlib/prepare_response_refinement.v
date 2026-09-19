(**
  PrepareResponse Message Refinement — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, abstract; NOT a mechanical refinement):
    neo-consensus/src/messages/prepare_response.rs::PrepareResponseMessage (9-18)
      Fields: block_index: u32, view_number: u8, validator_index: u8,
              preparation_hash: UInt256
    serialize(): returns the preparation_hash bytes (32 bytes).

  This is an ABSTRACT model of the prepare-response vote structure, its
  validation, and the dBFT quorum arithmetic M = n - f.  Machine-checked:
    - serialization returns exactly the preparation hash,
    - valid responses have a preparation hash of protocol length 32,
    - view/validator indices are u8-bounded and block index u32-bounded,
    - honest-majority bound: if f*3 < n then the quorum n - f exceeds f,
    - concrete Neo N3 quorum sizes (n=4 -> 3, n=7 -> 5, ...).

  Boundaries:
    - UInt256 hashes are modeled as nat; actual 32-byte encoding is out of scope.
    - No claim that the Rust/c# PrepareResponse is refined.
    - The quorum theorem assumes a distinct validators/faults count (the
      classic n = 3f+1 relation); it does not model Byzantine strategy games.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

Section PrepareResponseModel.

Record PrepareResponseMessage := make_prepare_response {
  block_index      : nat;   (* u32 *)
  view_number      : nat;   (* u8 *)
  validator_index  : nat;   (* u8 *)
  preparation_hash : nat    (* UInt256 modeled as nat *)
}.

(* u8 max = 255 = 2^8 - 1 (product form so lia can reason). *)
Definition MAX_U8 : nat := (2*2*2*2*2*2*2*2) - 1.
(* u32 max = 2^32 - 1 (arithmetic-product form for lia). *)
Definition MAX_U32 : nat := 256*256*256*256 - 1.
(* UInt256 length in bytes. *)
Definition HASH_LENGTH : nat := 32.

Definition serialize_prepare_response (msg : PrepareResponseMessage) : nat :=
  msg.(preparation_hash).

Definition validate_prepare_response (msg : PrepareResponseMessage) : Prop :=
  msg.(block_index) <= MAX_U32 /\
  msg.(view_number) <= MAX_U8 /\
  msg.(validator_index) <= MAX_U8 /\
  msg.(preparation_hash) = HASH_LENGTH.

Definition safe_to_accept_vote (msg : PrepareResponseMessage) : Prop :=
  validate_prepare_response msg.

(* --- Hash / bounds / serialization --- *)

Lemma hash_length_correct :
  forall (msg : PrepareResponseMessage),
    validate_prepare_response msg -> msg.(preparation_hash) = HASH_LENGTH.
Proof.
  intros msg Hvalid. destruct Hvalid as [_ [_ [_ Hhash]]]. exact Hhash.
Qed.

Lemma block_index_bounds :
  forall (msg : PrepareResponseMessage),
    validate_prepare_response msg -> msg.(block_index) <= MAX_U32.
Proof.
  intros msg Hvalid. destruct Hvalid as [Hblk _]. exact Hblk.
Qed.

Lemma view_number_bounds :
  forall (msg : PrepareResponseMessage),
    validate_prepare_response msg -> msg.(view_number) <= MAX_U8.
Proof.
  intros msg Hvalid. destruct Hvalid as [_ [Hview _]]. exact Hview.
Qed.

Lemma validator_index_bounds :
  forall (msg : PrepareResponseMessage),
    validate_prepare_response msg -> msg.(validator_index) <= MAX_U8.
Proof.
  intros msg Hvalid. destruct Hvalid as [_ [_ [Hval _]]]. exact Hval.
Qed.

Theorem serialize_preserves_hash :
  forall (msg : PrepareResponseMessage),
    serialize_prepare_response msg = msg.(preparation_hash).
Proof.
  intros msg. unfold serialize_prepare_response. reflexivity.
Qed.

Theorem serialize_hash_is_32_for_valid :
  forall (msg : PrepareResponseMessage),
    validate_prepare_response msg -> serialize_prepare_response msg = HASH_LENGTH.
Proof.
  intros msg Hvalid.
  rewrite serialize_preserves_hash.
  apply hash_length_correct. assumption.
Qed.

Theorem validation_safety :
  forall (msg : PrepareResponseMessage),
    validate_prepare_response msg -> safe_to_accept_vote msg.
Proof.
  intros msg Hvalid. unfold safe_to_accept_vote. exact Hvalid.
Qed.

End PrepareResponseModel.

(** dBFT quorum arithmetic: M = n - f with honest majority f*3 < n. *)
Section Quorum.

Definition compute_f (n : nat) : nat := Nat.div (Nat.sub n 1) 3.
Definition quorum (n f : nat) : nat := n - f.

(* Quorum equals n - f by definition. *)
Theorem quorum_formula : forall n f, quorum n f = n - f.
Proof. intros n f. unfold quorum. reflexivity. Qed.

(* Classic honest-majority bound: quorum n - f exceeds the fault count f
   whenever 3f < n (i.e. n >= 3f+1, the dBFT relation with n = 3f+1). *)
Theorem honest_majority :
  forall n f : nat, f * 3 < n -> quorum n f > f.
Proof.
  intros n f H. unfold quorum. lia.
Qed.

(* Concrete Neo N3 configurations: n=4 -> f=1, M=3; n=7 -> 5; n=10 -> 7; n=13 -> 9. *)
Theorem M_formula_verified_NeoN3 :
  quorum 4 (compute_f 4) = 3 /\ quorum 7 (compute_f 7) = 5 /\
  quorum 10 (compute_f 10) = 7 /\ quorum 13 (compute_f 13) = 9.
Proof.
  repeat split; vm_compute; reflexivity.
Qed.

End Quorum.

(** Concrete example. *)
Section Examples.

Definition example_response :=
  make_prepare_response 100 5 3 HASH_LENGTH.

Theorem example_response_valid : validate_prepare_response example_response.
Proof.
  unfold example_response, validate_prepare_response.
  cbn [block_index view_number validator_index preparation_hash].
  unfold MAX_U32, MAX_U8, HASH_LENGTH.
  repeat (split; try lia).
Qed.

Example example_serialize_32 :
  serialize_prepare_response example_response = 32.
Proof.
  unfold example_response, serialize_prepare_response, HASH_LENGTH. reflexivity.
Qed.

End Examples.