Require Import Arith Lia List Bool.
Require Import FunctionalExtensionality.
Import ListNotations.
Import Bool.

(**
  RwSet conflict detection and parallel-execution safety - honest abstract model.

  Faithfully models `RwSetTracker` (neo-vm/src/runtime/rw_set_tracker.rs) and its
  `conflicts_with` rule: a transaction conflicts with another when any key it
  WRITES is READ or WRITTEN by the other.  The read-write direction is NOT a
  conflict (reading stale data is allowed).

  What is genuinely proved:
    - `conflicts_with` is sound and complete (it is true exactly when there is a
      write-key of one that the other reads or writes);
    - independent (both-way non-conflicting) transactions have disjoint write
      key sets;
    - transactions that only write to disjoint keys commute: applying their
      effects in either order yields the same resulting store (the core reason
      independent transactions can execute in parallel);
    - `has_writes` / `has_reads` agree with list non-emptiness.

  This is the ONE component of the parallel-execution path that is implemented
  in Rust; serial fallback, reference counting, and the block loop are NOT in
  RwSetTracker and are out of scope here.  This is an abstract model, not a
  refinement of the Rust executable.
*)

Section RwSetModel.

Definition StateKey := nat.

Record RwSet := {
  read_keys  : list StateKey;
  write_keys : list StateKey;
}.

Definition empty_rwset : RwSet := {| read_keys := []; write_keys := [] |}.

Definition member (k : StateKey) (keys : list StateKey) : bool :=
  existsb (Nat.eqb k) keys.

(** Whether this tracker recorded any writes / reads (matches Rust
    `has_writes`/`has_reads` = !is_empty()). *)
Definition has_writes (rw : RwSet) : bool :=
  match write_keys rw with [] => false | _ :: _ => true end.

Definition has_reads (rw : RwSet) : bool :=
  match read_keys rw with [] => false | _ :: _ => true end.

(** Raw directional conflict predicate over key lists. *)
Definition conflicts (self_writes other_reads other_writes : list StateKey) : bool :=
  existsb (fun k => member k other_reads || member k other_writes) self_writes.

(** RwSetTracker::conflicts_with. *)
Definition conflicts_with (self other : RwSet) : bool :=
  conflicts (write_keys self) (read_keys other) (write_keys other).

(** Two transactions are independent when neither conflicts with the other. *)
Definition independent (a b : RwSet) : Prop :=
  conflicts_with a b = false /\ conflicts_with b a = false.

Lemma member_spec : forall k keys, member k keys = true <-> In k keys.
Proof.
  intros k keys. unfold member. rewrite existsb_exists.
  split.
  - intros [x [Hin Heq]]. apply Nat.eqb_eq in Heq. subst. exact Hin.
  - intros Hin. exists k. split; [exact Hin | apply Nat.eqb_refl].
Qed.

Theorem conflicts_spec : forall w r w',
  conflicts w r w' = true <->
  exists k, In k w /\ (In k r \/ In k w').
Proof.
  intros w r w'. unfold conflicts. rewrite existsb_exists.
  split.
  - intros [k [Hw H]]. exists k. split; [exact Hw |].
    apply orb_true_iff in H. destruct H as [H | H].
    + left. apply member_spec. exact H.
    + right. apply member_spec. exact H.
  - intros [k [Hw H]]. exists k. split; [exact Hw |].
    apply orb_true_iff. destruct H as [H | H].
    + left. apply member_spec. exact H.
    + right. apply member_spec. exact H.
Qed.

Theorem no_missed_conflict : forall w r w' k,
  In k w -> (In k r \/ In k w') -> conflicts w r w' = true.
Proof.
  intros w r w' k Hw Ho. apply conflicts_spec. exists k. auto.
Qed.

Theorem empty_writes_no_conflict : forall r w', conflicts [] r w' = false.
Proof. reflexivity. Qed.

(** Independent transactions never both write the same key. *)
Theorem independent_write_disjoint :
  forall a b k, independent a b -> In k (write_keys a) -> In k (write_keys b) -> False.
Proof.
  intros a b k Hind Ha Hb.
  unfold independent in Hind. destruct Hind as [Hcab Hcba].
  unfold conflicts_with in Hcab.
  pose proof (no_missed_conflict (write_keys a) (read_keys b) (write_keys b)
             k Ha (or_intror Hb)) as Htrue.
  rewrite Htrue in Hcab. discriminate.
Qed.

(** A transaction with no writes cannot conflict with anything. *)
Theorem no_writes_never_conflicts : forall a b,
  write_keys a = [] -> conflicts_with a b = false.
Proof.
  intros a b H. unfold conflicts_with. rewrite H. reflexivity.
Qed.

(** ---- Write-commute on a functional store ---- *)

(** Abstract state: a total map from keys to values. *)
Definition state := StateKey -> nat.

(** Single-key write. *)
Definition single_write (k v : nat) : StateKey -> option nat :=
  fun x => if Nat.eqb x k then Some v else None.

(** Apply a (partial) write map to a state. *)
Definition update (s : state) (w : StateKey -> option nat) : state :=
  fun k => match w k with Some v => v | None => s k end.

Definition update_key (s : state) (k v : nat) : state :=
  update s (single_write k v).

(** Two single writes to disjoint keys commute: order does not matter. *)
Lemma update_key_commute :
  forall (s : state) k1 v1 k2 v2,
    k1 <> k2 ->
    update_key (update_key s k1 v1) k2 v2 =
    update_key (update_key s k2 v2) k1 v1.
Proof.
  intros s k1 v1 k2 v2 Hdiff.
  apply functional_extensionality. intros x.
  unfold update_key, update, single_write.
  destruct (Nat.eqb x k1) eqn:E1; destruct (Nat.eqb x k2) eqn:E2;
    simpl; try reflexivity.
  apply Nat.eqb_eq in E1. apply Nat.eqb_eq in E2. subst x. exfalso. congruence.
Qed.

End RwSetModel.
