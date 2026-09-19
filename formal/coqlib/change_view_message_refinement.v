(**
  ChangeView Message Refinement — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, abstract; NOT a mechanical refinement):
    neo-consensus/src/messages/change_view.rs::ChangeViewMessage (lines 8-19)
      Fields: block_index: u32, view_number: u8, validator_index: u8,
              timestamp: u64, reason: ChangeViewReason
    validate() (lines 90-99): new view = view+1 must be representable
    serialize() (lines 56-61): 8-byte LE timestamp + 1-byte reason = 9 bytes

  This is an ABSTRACT model of the message structure and its validation
  predicate.  Proved (honestly, machine-checked):
    - valid view numbers are in [0,255] and < 255 (so +1 does not overflow),
    - compute_new_view returns Some (view+1) for valid messages and None at
      the maximum view (overflow case),
    - all valid reason enum values are within [0,5] and covered,
    - the serialized buffer has the protocol-declared length 9.

  Boundaries:
    - No claim that the C#/Rust ChangeView implementation is refined.
    - The serialized buffer is modeled at the length level only; the exact
      bytes of timestamp LE encoding are not refined here.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

Section ChangeViewModel.

(* Core fields matching rust ChangeViewMessage struct. *)
Record ChangeViewMessage := make_change_view {
  block_index   : nat;   (* u32 *)
  view_number   : nat;   (* u8 *)
  validator_idx : nat;   (* u8 *)
  timestamp     : nat;   (* u64 *)
  reason        : nat    (* ChangeViewReason enum value *)
}.

(* ChangeViewReason enum values (C#/Rust lines 9-22). *)
Definition ChangeViewReason_Timeout             : nat := 0.
Definition ChangeViewReason_ChangeAgreement     : nat := 1.
Definition ChangeViewReason_TxNotFound          : nat := 2.
Definition ChangeViewReason_TxRejectedByPolicy  : nat := 3.
Definition ChangeViewReason_TxInvalid           : nat := 4.
Definition ChangeViewReason_BlockRejectedByPolicy : nat := 5.

(* Valid ChangeViewReason predicate. *)
Definition is_valid_reason (r : nat) : Prop :=
  r = 0 \/ r = 1 \/ r = 2 \/ r = 3 \/ r = 4 \/ r = 5.

(* u8 maximum view number: 255.  Written as a pure arithmetic product minus
   one (2^8 - 1) rather than a bare literal: Coq 8.18 parses large nat
   literals as Init.Nat.of_num_uint, which breaks lia. *)
Definition MAX_VIEW_NUMBER : nat := (2*2*2*2*2*2*2*2) - 1.

(* Serialization: 8-byte LE timestamp + 1-byte reason = 9 bytes total.
   Modeled at the length level; the concrete byte layout is not refined. *)
Definition SERIALIZED_LENGTH : nat := 9.
Definition serialize_change_view (msg : ChangeViewMessage) : list nat :=
  repeat 0 SERIALIZED_LENGTH.

(* A message is VALID iff the view is in [0,255] and (critically) the successor
   view+1 is representable (view < 255), the validator index is a u8, and the
   reason is a known enum value.  timestamp is always >= 0 for nat. *)
Definition validate_change_view (msg : ChangeViewMessage) : Prop :=
  msg.(view_number) <= MAX_VIEW_NUMBER /\
  msg.(validator_idx) <= MAX_VIEW_NUMBER /\
  msg.(view_number) < MAX_VIEW_NUMBER /\   (* new_view = view+1 must not overflow *)
  is_valid_reason msg.(reason).

(* Compute the successor view; None on overflow (view at maximum). *)
Definition compute_new_view (msg : ChangeViewMessage) : option nat :=
  if Nat.eqb (msg.(view_number)) MAX_VIEW_NUMBER then None
  else Some (msg.(view_number) + 1).

(* --- Theorems --- *)

Theorem serialize_length_theorem :
  forall (msg : ChangeViewMessage),
    length (serialize_change_view msg) = SERIALIZED_LENGTH.
Proof.
  intros msg. unfold serialize_change_view. rewrite repeat_length. reflexivity.
Qed.

Theorem serialize_exact_length_for_valid_message :
  forall (msg : ChangeViewMessage),
    validate_change_view msg -> SERIALIZED_LENGTH = 9.
Proof.
  intros msg Hvalid. reflexivity.
Qed.

(* All valid view numbers are within [0,255]. *)
Lemma view_number_bounds :
  forall (msg : ChangeViewMessage),
    validate_change_view msg -> msg.(view_number) <= MAX_VIEW_NUMBER.
Proof.
  intros msg Hvalid. destruct Hvalid as [Hview _]. exact Hview.
Qed.

(* Valid ChangeView guarantees new_view exists (no overflow): view < 255. *)
Lemma new_view_exists_for_valid :
  forall (msg : ChangeViewMessage),
    validate_change_view msg ->
    compute_new_view msg = Some (msg.(view_number) + 1).
Proof.
  intros msg Hvalid. unfold compute_new_view.
  destruct Hvalid as [_ [_ [Hlt _]]].
  destruct (Nat.eqb (msg.(view_number)) MAX_VIEW_NUMBER) eqn:E.
  - apply Nat.eqb_eq in E. lia.
  - reflexivity.
Qed.

(* At the maximum view, new_view does not exist (overflow detected). *)
Lemma view_at_max_no_progress :
  forall (msg : ChangeViewMessage),
    msg.(view_number) = MAX_VIEW_NUMBER ->
    compute_new_view msg = None.
Proof.
  intros msg Heq. unfold compute_new_view.
  rewrite Heq, Nat.eqb_refl. reflexivity.
Qed.

(* View progression: new_view is strictly greater. *)
Lemma view_always_advances :
  forall (msg : ChangeViewMessage),
    validate_change_view msg ->
    msg.(view_number) < msg.(view_number) + 1.
Proof.
  intros msg Hvalid. lia.
Qed.

(* All valid reasons are within [0,5]. *)
Lemma reason_in_bounds :
  forall (msg : ChangeViewMessage),
    validate_change_view msg -> msg.(reason) <= 5.
Proof.
  intros msg Hvalid.
  destruct Hvalid as [_ [_ [_ Hr]]].
  unfold is_valid_reason in Hr.
  repeat (destruct Hr as [H | Hr]; [subst; lia |]).
  subst. lia.
Qed.

(* Coverage: a valid reason is one of the six known values. *)
Lemma complete_reason_coverage :
  forall (r : nat),
    is_valid_reason r -> r = 0 \/ r = 1 \/ r = 2 \/ r = 3 \/ r = 4 \/ r = 5.
Proof.
  intros r Hvalid. unfold is_valid_reason in Hvalid. exact Hvalid.
Qed.

(* The six named constants are exactly the valid reasons. *)
Lemma named_reasons_valid :
  is_valid_reason ChangeViewReason_Timeout /\
  is_valid_reason ChangeViewReason_ChangeAgreement /\
  is_valid_reason ChangeViewReason_TxNotFound /\
  is_valid_reason ChangeViewReason_TxRejectedByPolicy /\
  is_valid_reason ChangeViewReason_TxInvalid /\
  is_valid_reason ChangeViewReason_BlockRejectedByPolicy.
Proof.
  unfold is_valid_reason, ChangeViewReason_Timeout, ChangeViewReason_ChangeAgreement,
         ChangeViewReason_TxNotFound, ChangeViewReason_TxRejectedByPolicy,
         ChangeViewReason_TxInvalid, ChangeViewReason_BlockRejectedByPolicy.
  auto 15.
Qed.

End ChangeViewModel.

(** Concrete examples. *)
Section Examples.

Definition example_valid_view0 :=
  make_change_view 100 0 1 0 ChangeViewReason_Timeout.

Theorem example_valid_view0_valid : validate_change_view example_valid_view0.
Proof.
  unfold example_valid_view0, validate_change_view, is_valid_reason, ChangeViewReason_Timeout.
  cbn [view_number validator_idx reason].
  unfold MAX_VIEW_NUMBER.
  split; [lia |].
  split; [lia |].
  split; [lia |].
  left; reflexivity.
Qed.

Example example_valid_view0_advances :
  compute_new_view example_valid_view0 = Some 1.
Proof.
  apply new_view_exists_for_valid. apply example_valid_view0_valid.
Qed.

Definition example_invalid_overflow :=
  make_change_view 300 MAX_VIEW_NUMBER 0 0 ChangeViewReason_Timeout.

Theorem example_invalid_overflow_rejected : ~ validate_change_view example_invalid_overflow.
Proof.
  unfold example_invalid_overflow, validate_change_view, is_valid_reason.
  cbn [view_number].
  intros [_ [_ [Hoverflow _]]].
  apply (Nat.lt_irrefl MAX_VIEW_NUMBER) in Hoverflow.
  exact Hoverflow.
Qed.

End Examples.