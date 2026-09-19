(**
  PrepareRequest Message Refinement — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, abstract; NOT a mechanical refinement):
    neo-consensus/src/messages/prepare_request.rs::PrepareRequestMessage (10-27)
      Fields: block_index: u32, view_number: u8, validator_index: u8, version: u32,
              prev_hash: UInt256, timestamp: u64, nonce: u64, transaction_hashes: Vec<UInt256>
    validate(): version == 0 (Neo N3), no duplicate transaction hashes.

  This is an ABSTRACT model of the prepare-request structure and validation.
  Machine-checked:
    - serialization returns exactly the transaction-hash list,
    - valid requests have at most MAX_TRANSACTIONS_PER_BLOCK hashes,
    - valid requests have pairwise-unique (no duplicate) transaction hashes,
    - view/validator index are u8-bounded, version is 0.

  Boundaries:
    - No claim that the Rust/c# PrepareRequest is refined.
    - UInt256 hashes are modeled as `nat`; actual 32-byte encoding is out of scope.
    - The injectivity lemma that would follow from no_duplicates for arbitrary
      index positions is deliberately not asserted; only the structural
      no_duplicates facts below are proved.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

Section PrepareRequestModel.

Record PrepareRequestMessage := make_prepare_request {
  block_index        : nat;
  view_number        : nat;
  validator_index    : nat;
  version            : nat;
  prev_hash          : nat;
  timestamp          : nat;
  nonce              : nat;
  transaction_hashes : list nat
}.

(* MAX_TRANSACTIONS_PER_BLOCK = 512 = 2^9 (Neo N3). Product form keeps lia working. *)
Definition MAX_TRANSACTIONS_PER_BLOCK : nat := (2*2*2*2*2*2*2*2*2).
(* u8 maximum = 255 = 2^8 - 1 (product form for lia). *)
Definition MAX_U8 : nat := (2*2*2*2*2*2*2*2) - 1.

(* Pairwise uniqueness of transaction hashes. *)
Fixpoint no_duplicates (ls : list nat) : Prop :=
  match ls with
  | [] => True
  | x :: xs => (~ In x xs) /\ no_duplicates xs
  end.

(* Validation: view and validator are u8-bounded, version is 0, hashes unique,
   and the transaction count does not exceed the protocol maximum.
   timestamp >= 0 always holds for nat. *)
Definition validate_prepare_request (msg : PrepareRequestMessage) : Prop :=
  msg.(view_number) <= MAX_U8 /\
  msg.(validator_index) <= MAX_U8 /\
  msg.(version) = 0 /\
  no_duplicates msg.(transaction_hashes) /\
  length msg.(transaction_hashes) <= MAX_TRANSACTIONS_PER_BLOCK /\
  msg.(timestamp) >= 0.

Definition serialize_prepare_request (msg : PrepareRequestMessage) : list nat :=
  msg.(transaction_hashes).

Definition safe_to_accept_proposal (msg : PrepareRequestMessage) : Prop :=
  validate_prepare_request msg.

Definition transaction_count (msg : PrepareRequestMessage) : nat :=
  length msg.(transaction_hashes).

(* --- Theorems --- *)

Lemma transaction_list_bound :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg ->
    length msg.(transaction_hashes) <= MAX_TRANSACTIONS_PER_BLOCK.
Proof.
  intros msg Hvalid.
  destruct Hvalid as [_ [_ [_ [_ [Hmax _]]]]]. exact Hmax.
Qed.

Lemma hash_correspondence_unique :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg -> no_duplicates msg.(transaction_hashes).
Proof.
  intros msg Hvalid.
  destruct Hvalid as [_ [_ [_ [Hdup _]]]]. exact Hdup.
Qed.

(* Structural no_duplicates facts (the honest, provable content). *)
Lemma no_duplicates_cons :
  forall x xs, no_duplicates (x :: xs) -> ~ In x xs.
Proof.
  intros x xs H. destruct H as [H _]. exact H.
Qed.

Lemma no_duplicates_no_immediate_dup :
  forall x xs, no_duplicates (x :: x :: xs) -> False.
Proof.
  intros x xs H.
  apply (no_duplicates_cons x (x :: xs)) in H.
  apply H. left. reflexivity.
Qed.

Theorem serialize_preserves_transaction_order :
  forall (msg : PrepareRequestMessage),
    serialize_prepare_request msg = msg.(transaction_hashes).
Proof.
  intros msg. unfold serialize_prepare_request. reflexivity.
Qed.

Theorem serialize_length_for_valid_message :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg ->
    length (serialize_prepare_request msg) = length msg.(transaction_hashes).
Proof.
  intros msg Hvalid. rewrite serialize_preserves_transaction_order. reflexivity.
Qed.

Lemma view_number_bounds :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg -> msg.(view_number) <= MAX_U8.
Proof.
  intros msg Hvalid. destruct Hvalid as [Hview _]. exact Hview.
Qed.

Lemma validator_index_bounds :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg -> msg.(validator_index) <= MAX_U8.
Proof.
  intros msg Hvalid. destruct Hvalid as [_ [Hval _]]. exact Hval.
Qed.

Lemma version_is_zero :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg -> msg.(version) = 0.
Proof.
  intros msg Hvalid. destruct Hvalid as [_ [_ [Hver _]]]. exact Hver.
Qed.

Theorem validation_safety :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg -> safe_to_accept_proposal msg.
Proof.
  intros msg Hvalid. unfold safe_to_accept_proposal. exact Hvalid.
Qed.

Corollary invalid_unsafe :
  forall (msg : PrepareRequestMessage),
    ~ validate_prepare_request msg -> ~ safe_to_accept_proposal msg.
Proof.
  intros msg Hinvalid Hsafe.
  apply Hinvalid. unfold safe_to_accept_proposal in Hsafe. exact Hsafe.
Qed.

Theorem block_size_safety :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg ->
    length msg.(transaction_hashes) <= MAX_TRANSACTIONS_PER_BLOCK.
Proof.
  intros msg Hvalid. apply transaction_list_bound. assumption.
Qed.

Theorem duplicate_prevention :
  forall (msg : PrepareRequestMessage),
    validate_prepare_request msg -> no_duplicates msg.(transaction_hashes).
Proof.
  intros msg Hvalid. apply hash_correspondence_unique. assumption.
Qed.

End PrepareRequestModel.

(** Concrete examples. *)
Section Examples.

Definition example_req :=
  make_prepare_request 100 0 1 0 0 0 0 [1; 2; 3].

Theorem example_req_valid : validate_prepare_request example_req.
Proof.
  unfold example_req, validate_prepare_request.
  cbn [view_number validator_index version transaction_hashes timestamp].
  unfold MAX_U8, MAX_TRANSACTIONS_PER_BLOCK.
  repeat (split; try lia); cbn; intuition; try lia.
Qed.

(* Duplicate hashes are rejected. *)
Definition example_dup :=
  make_prepare_request 100 0 1 0 0 0 0 [1; 2; 1].

Theorem example_dup_rejected : ~ validate_prepare_request example_dup.
Proof.
  unfold example_dup, validate_prepare_request.
  cbn [view_number validator_index version transaction_hashes timestamp].
  unfold MAX_U8, MAX_TRANSACTIONS_PER_BLOCK.
  intros [_ [_ [_ [Hdup _]]]].
  apply (no_duplicates_cons 1 (2 :: 1 :: nil)) in Hdup.
  apply Hdup. cbn. auto.
Qed.

(* An empty transaction list is allowed (coinbase-only block). *)
Theorem empty_tx_list_valid :
  forall (msg : PrepareRequestMessage),
    msg.(view_number) <= MAX_U8 ->
    msg.(validator_index) <= MAX_U8 ->
    msg.(version) = 0 ->
    msg.(transaction_hashes) = [] ->
    validate_prepare_request msg.
Proof.
  intros msg Hview Hval Hver Hempty.
  unfold validate_prepare_request.
  rewrite Hempty. simpl.
  split; [assumption |].
  split; [assumption |].
  split; [assumption |].
  split; [exact I |].
  split; lia.
Qed.

End Examples.