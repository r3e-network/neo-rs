From Coq Require Import List Bool Arith.
Import ListNotations.

(* Directional dependency predicate matching RwSetTracker::conflicts_with.
   Keys are abstract naturals; this is not a translation of Rust machine code. *)
Definition member (key : nat) (keys : list nat) : bool :=
  existsb (Nat.eqb key) keys.

Definition conflicts (writes reads_other writes_other : list nat) : bool :=
  existsb (fun k => member k reads_other || member k writes_other) writes.

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

Theorem directional_counterexample :
  conflicts [1] [1] [] = true /\ conflicts [] [] [1] = false.
Proof. split; reflexivity. Qed.

Print Assumptions conflicts_spec.
Print Assumptions no_missed_conflict.
Print Assumptions empty_writes_no_conflict.
Print Assumptions directional_counterexample.
