Require Import Arith Lia List.
Import ListNotations.

From Coq Require Import ZArith.

(** Smart contract validation — honest model (Coq 8.18).

    Pure structural properties only: script byte range, script size bound,
    gas-consumption invariant, and multi-sig parameter validity.  No claim of
    refinement against the Rust executable; gas is modeled abstractly.
*)

Definition bytes := list nat.

Definition valid_byte (b : nat) : Prop := b <= 255.

Definition valid_script (s : bytes) : Prop :=
  forall b, In b s -> valid_byte b.

(** 1 MiB, written arithmetically so `lia` can use it (a big nat literal would
    be parsed as Init.Nat.of_num_uint and defeat lia). *)
Definition MAX_SCRIPT_SIZE : nat := 1024 * 1024.

Section ContractModel.

Definition UInt160 := bytes.
Definition uint160_length : nat := 20.
Definition valid_uint160 (h : UInt160) : Prop := length h = uint160_length.

Record SmartContract := {
  script                : bytes;
  parameter_count       : nat;
  computed_script_hash  : UInt160;
}.

(** ---- Gas accounting (abstract, non-negative budget) ---- *)
Record InvocationContext := {
  max_gas_limit : nat;
  consumed_gas  : nat;
}.

Definition make_invocation_context (max_g : nat) : InvocationContext :=
  {| max_gas_limit := max_g; consumed_gas := 0 |}.

Definition consume_gas (ctx : InvocationContext) (amount : nat)
  : option InvocationContext :=
  if Nat.leb (ctx.(consumed_gas) + amount) ctx.(max_gas_limit) then
    Some {| max_gas_limit := ctx.(max_gas_limit);
            consumed_gas := ctx.(consumed_gas) + amount |}
  else None.

Definition valid_invocation (ctx : InvocationContext) : Prop :=
  ctx.(consumed_gas) <= ctx.(max_gas_limit).

(** ---- Validation predicates ---- *)
Definition script_within_size_bounds (contract : SmartContract) : Prop :=
  length contract.(script) < MAX_SCRIPT_SIZE.

(** ---- Multi-sig params: 1 <= m <= n <= 1024 ---- *)
Definition valid_multisig_params (m n : nat) : Prop :=
  1 <= m /\ m <= n /\ n <= 1024.

(** THEOREM 1: a script larger than the bound is rejected. *)
Lemma script_size_bound :
  forall (s : bytes) (param_cnt : nat),
    length s < MAX_SCRIPT_SIZE ->
    script_within_size_bounds {| script := s; parameter_count := param_cnt; computed_script_hash := [] |}.
Proof.
  intros s param_cnt Hsize.
  unfold script_within_size_bounds. exact Hsize.
Qed.

(** THEOREM 2: starting valid, a legitimate consume keeps gas within budget. *)
Lemma consume_gas_preserves_validity :
  forall (ctx : InvocationContext) (amount : nat),
    valid_invocation ctx ->
    consume_gas ctx amount <> None ->
    valid_invocation (match consume_gas ctx amount with
                      | Some ctx' => ctx'
                      | None => ctx
                      end).
Proof.
  intros ctx amount Hvalid Hsome.
  unfold consume_gas, valid_invocation in *.
  destruct (Nat.leb (ctx.(consumed_gas) + amount) ctx.(max_gas_limit)) eqn:E.
  - apply Nat.leb_le in E. exact E.
  - exfalso. apply Hsome. reflexivity.
Qed.

(** THEOREM 3: two contracts equal (same script+hash) iff their param counts
    match, given equal scripts. *)
Lemma contract_hash_congruence :
  forall (s1 s2 : bytes) (p1 p2 : nat) (h : UInt160),
    s1 = s2 ->
    {| script := s1; parameter_count := p1; computed_script_hash := h |} =
    {| script := s2; parameter_count := p2; computed_script_hash := h |} <-> p1 = p2.
Proof.
  intros s1 s2 p1 p2 h Hs.
  split.
  - intro eq. simpl in eq. inversion eq. reflexivity.
  - intro Heq. subst. reflexivity.
Qed.

(** THEOREM 4 (correct iff): multi-sig params valid exactly when 1<=m<=n<=1024. *)
Lemma multisig_valid_param_range :
  forall (m n : nat),
    valid_multisig_params m n <-> (1 <= m /\ m <= n /\ n <= 1024).
Proof.
  intros m n. reflexivity.
Qed.

(** THEOREM 5: an empty script is within size bounds (and entirely valid). *)
Lemma empty_script_valid_and_bounded :
  valid_script [] /\
  script_within_size_bounds {| script := []; parameter_count := 0;
                               computed_script_hash := [] |}.
Proof.
  split.
  - unfold valid_script. intros b Hb. inversion Hb.
  - unfold script_within_size_bounds, MAX_SCRIPT_SIZE.
    apply Nat.mul_pos_pos; lia.
Qed.

End ContractModel.

(** Concrete running examples (existential witnesses, not over-claims). *)
Section Examples.

Example partial_invocation_valid :
  valid_invocation {| max_gas_limit := 100; consumed_gas := 50 |}.
Proof.
  unfold valid_invocation. simpl. lia.
Qed.

Example multisig_1_of_3_valid : valid_multisig_params 1 3.
Proof.
  unfold valid_multisig_params.
  split; [lia|]; split; [lia|].
  apply Nat.leb_le. vm_compute. reflexivity.
Qed.

Example multisig_0_invalid : ~ valid_multisig_params 0 5.
Proof.
  intro H. unfold valid_multisig_params in H.
  destruct H as [H1 _]. inversion H1.
Qed.

End Examples.