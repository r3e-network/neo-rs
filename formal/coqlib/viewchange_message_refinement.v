(**
  ViewChange Message Protocol Semantics — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, abstract; NOT a mechanical refinement):
    neo-consensus/src/messages/change_view.rs::ChangeViewMessage (8-19)
      new_view_number() (41-45): view + 1 or overflow error
      validate() (90-99): view progression safety
      serialize() (56-61): Neo N3 DBFTPlugin format (9 bytes)

  This is an ABSTRACT model of the view-change protocol semantics: view
  progression (strict increase, no overflow), timestamp bounds under the
  protocol timeout/drift constants, validator-index authentication, and the
  non-negativity of the (abstract) ledger/hash state fields.  Machine-checked:
    - valid view numbers are u8-bounded and strictly < MAX (successor exists),
    - new_view = view+1 for valid messages; None at the maximum view,
    - the successor view is strictly greater than the current one,
    - timestamps respect the bound view*drift + timeout,
    - an authenticated validator index is < total when MAX_U8 < total.

  Boundaries:
    - No claim that the Rust/c# ChangeView implementation is refined.
    - ledger/hash are modeled as opaque nat placeholders; "correspondence"
      is not claimed beyond non-negativity.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

Section ViewChangeModel.

Record ViewChangeMessage := make_view_change {
  block_index     : nat;
  view_number     : nat;
  validator_idx   : nat;
  timestamp       : nat;
  reason          : nat;
  last_block_hash : nat;
  ledger_state    : nat
}.

Definition VCReason_Timeout : nat := 0.
Definition VCReason_ChangeAgreement : nat := 1.
Definition VCReason_TxNotFound : nat := 2.
Definition VCReason_TxRejectedByPolicy : nat := 3.
Definition VCReason_TxInvalid : nat := 4.
Definition VCReason_BlockRejectedByPolicy : nat := 5.

Definition is_valid_vc_reason (r : nat) : Prop :=
  r = 0 \/ r = 1 \/ r = 2 \/ r = 3 \/ r = 4 \/ r = 5.

(* u8 max = 255 (2^8 - 1, product form for lia). *)
Definition MAX_VIEW_NUMBER : nat := (2*2*2*2*2*2*2*2) - 1.
Definition MAX_VALIDATOR_INDEX : nat := (2*2*2*2*2*2*2*2) - 1.
(* Protocol time constants as pure arithmetic products (lia-friendly). *)
Definition VIEW_CHANGE_TIMEOUT : nat := 4 * 10 * 10 * 10.
Definition MAX_TIMESTAMP_DRIFT : nat := 3 * 10 * 10 * 10 * 10.

Definition validate_view_change (msg : ViewChangeMessage) : Prop :=
  msg.(view_number) <= MAX_VIEW_NUMBER /\
  msg.(validator_idx) <= MAX_VALIDATOR_INDEX /\
  msg.(view_number) < MAX_VIEW_NUMBER /\
  msg.(timestamp) <= msg.(view_number) * MAX_TIMESTAMP_DRIFT + VIEW_CHANGE_TIMEOUT /\
  is_valid_vc_reason msg.(reason) /\
  msg.(last_block_hash) >= 0 /\
  msg.(ledger_state) >= 0.

Definition compute_next_view (msg : ViewChangeMessage) : option nat :=
  if Nat.eqb (msg.(view_number)) MAX_VIEW_NUMBER then None
  else Some (msg.(view_number) + 1).

(* View-change is safe exactly when it validates. *)
Definition view_change_is_safe (msg : ViewChangeMessage) : Prop :=
  validate_view_change msg.

(* Timeout validity: timestamp within the acceptable protocol window. *)
Definition timeout_is_valid (msg : ViewChangeMessage) : Prop :=
  msg.(timestamp) <= (msg.(view_number) + 1) * VIEW_CHANGE_TIMEOUT + MAX_TIMESTAMP_DRIFT.

(* Validator is a participant (index below the total validator count). *)
Definition validator_is_authenticated (total_validators : nat)
                                      (msg : ViewChangeMessage) : Prop :=
  msg.(validator_idx) < total_validators.

(* --- View progression theorems --- *)

Lemma view_number_bounds :
  forall (msg : ViewChangeMessage),
    validate_view_change msg -> msg.(view_number) <= MAX_VIEW_NUMBER.
Proof.
  intros msg Hvalid. destruct Hvalid as [Hv _]. exact Hv.
Qed.

(* No view regression: old < new for the successor mapping. *)
Lemma no_view_regression :
  forall (old_view new_view : nat),
    old_view < MAX_VIEW_NUMBER ->
    new_view = old_view + 1 ->
    old_view < new_view.
Proof.
  intros old_view new_view Hbound Heq. subst. lia.
Qed.

(* New view always exists for non-maximum views. *)
Lemma new_view_exists :
  forall (msg : ViewChangeMessage),
    validate_view_change msg ->
    compute_next_view msg = Some (msg.(view_number) + 1).
Proof.
  intros msg Hvalid. unfold compute_next_view.
  destruct Hvalid as [_ [_ [Hlt _]]].
  destruct (Nat.eqb (msg.(view_number)) MAX_VIEW_NUMBER) eqn:E.
  - apply Nat.eqb_eq in E. lia.
  - reflexivity.
Qed.

(* Maximum view cannot advance (overflow protection). *)
Lemma max_view_no_progress :
  forall (msg : ViewChangeMessage),
    msg.(view_number) = MAX_VIEW_NUMBER ->
    compute_next_view msg = None.
Proof.
  intros msg Heq. unfold compute_next_view.
  rewrite Heq, Nat.eqb_refl. reflexivity.
Qed.

(* View progression: successor is strictly greater. *)
Lemma view_progression_strict :
  forall (msg : ViewChangeMessage),
    validate_view_change msg ->
    msg.(view_number) < msg.(view_number) + 1.
Proof.
  intros msg Hvalid. lia.
Qed.

Theorem validation_iff_safety :
  forall (msg : ViewChangeMessage),
    validate_view_change msg <-> view_change_is_safe msg.
Proof.
  split.
  - intros H. unfold view_change_is_safe. exact H.
  - intros H. unfold view_change_is_safe in H. exact H.
Qed.

(* --- Timestamp bounds --- *)

Lemma timeout_bounds_valid :
  forall (msg : ViewChangeMessage),
    validate_view_change msg ->
    msg.(timestamp) <= msg.(view_number) * MAX_TIMESTAMP_DRIFT + VIEW_CHANGE_TIMEOUT.
Proof.
  intros msg Hvalid.
  destruct Hvalid as [_ [_ [_ [Htime _]]]]. exact Htime.
Qed.

(* --- Validator authentication --- *)

Lemma validator_auth_from_validate :
  forall (msg : ViewChangeMessage) (total : nat),
    validate_view_change msg ->
    MAX_VALIDATOR_INDEX < total ->
    validator_is_authenticated total msg.
Proof.
  intros msg total Hvalid Htotal.
  destruct Hvalid as [_ [Hidx _]].
  unfold validator_is_authenticated.
  unfold MAX_VALIDATOR_INDEX in *.
  lia.
Qed.

(* --- Ledger/hash state are non-negative (nat is always >= 0). --- *)

Lemma last_block_hash_geq_zero :
  forall (msg : ViewChangeMessage),
    validate_view_change msg -> msg.(last_block_hash) >= 0.
Proof.
  intros msg Hvalid. destruct Hvalid as [_ [_ [_ [_ [_ [Hhash _]]]]]]. exact Hhash.
Qed.

Lemma ledger_state_geq_zero :
  forall (msg : ViewChangeMessage),
    validate_view_change msg -> msg.(ledger_state) >= 0.
Proof.
  intros msg Hvalid.
  destruct Hvalid as [_ [_ [_ [_ [_ [_ Hstate]]]]]]. exact Hstate.
Qed.

End ViewChangeModel.

(** Concrete examples. *)
Section Examples.

Definition example_valid_view0 :=
  make_view_change 100 0 1 0 VCReason_Timeout 5 6.

Theorem example_valid_view0_valid : validate_view_change example_valid_view0.
Proof.
  unfold example_valid_view0, validate_view_change, is_valid_vc_reason, VCReason_Timeout.
  cbn [view_number validator_idx timestamp reason last_block_hash ledger_state].
  unfold MAX_VIEW_NUMBER, MAX_VALIDATOR_INDEX, MAX_TIMESTAMP_DRIFT, VIEW_CHANGE_TIMEOUT.
  split; [lia |].
  split; [lia |].
  split; [lia |].
  split; [lia |].
  split.
  - left; reflexivity.
  - split; lia.
Qed.

Example example_view0_advances : compute_next_view example_valid_view0 = Some 1.
Proof.
  apply new_view_exists. apply example_valid_view0_valid.
Qed.

Definition example_invalid_overflow :=
  make_view_change 300 MAX_VIEW_NUMBER 0 0 VCReason_Timeout 0 0.

Theorem example_invalid_overflow_rejected : ~ validate_view_change example_invalid_overflow.
Proof.
  unfold example_invalid_overflow, validate_view_change, is_valid_vc_reason.
  cbn [view_number].
  intros [_ [_ [Hoverflow _]]].
  apply (Nat.lt_irrefl MAX_VIEW_NUMBER) in Hoverflow.
  exact Hoverflow.
Qed.

End Examples.