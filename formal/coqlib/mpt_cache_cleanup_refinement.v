From Coq Require Import List Arith Bool Lia.
Import ListNotations.

(* Abstract MRU-first LRU list. No claim about Arc counters, persisted MPT
   reference counts, allocator behavior, or refinement of the lru crate. *)
Definition Entry := (nat * nat)%type.
Definition CacheState := list Entry.
Definition remove_key (k : nat) (cs : CacheState) : CacheState :=
  filter (fun e => negb (Nat.eqb k (fst e))) cs.
Definition cache_put (capacity k value : nat) (cs : CacheState) : CacheState :=
  firstn capacity ((k, value) :: remove_key k cs).
Definition evict_oldest (cs : CacheState) : CacheState :=
  firstn (length cs - 1) cs.

Theorem capacity_enforced : forall capacity k value cs,
  length (cache_put capacity k value cs) <= capacity.
Proof.
  intros. unfold cache_put. rewrite firstn_length. apply Nat.le_min_l.
Qed.

Theorem oldest_eviction_length : forall cs,
  cs <> [] -> length (evict_oldest cs) = length cs - 1.
Proof.
  intros cs H. unfold evict_oldest. rewrite firstn_length.
  apply Nat.min_l. lia.
Qed.

Theorem eviction_preserves_recent_prefix : forall cs,
  evict_oldest cs = firstn (length cs - 1) cs.
Proof. reflexivity. Qed.

Theorem removes_old_value : forall k cs e,
  In e (remove_key k cs) -> fst e <> k.
Proof.
  intros k cs e H. unfold remove_key in H.
  apply filter_In in H. destruct H as [_ H].
  apply negb_true_iff in H. apply Nat.eqb_neq in H. congruence.
Qed.

Theorem new_entry_is_most_recent : forall capacity k value cs,
  capacity > 0 -> hd_error (cache_put capacity k value cs) = Some (k, value).
Proof.
  intros capacity k value cs H. destruct capacity; [lia | reflexivity].
Qed.

Theorem retained_entries_have_provenance : forall capacity k value cs e,
  In e (cache_put capacity k value cs) ->
  e = (k, value) \/ (In e cs /\ fst e <> k).
Proof.
  intros capacity k value cs e H. unfold cache_put in H.
  assert (Hin : In e ((k, value) :: remove_key k cs)).
  { assert (Hfull : In e
        (firstn capacity ((k, value) :: remove_key k cs) ++
         skipn capacity ((k, value) :: remove_key k cs))).
    { apply in_or_app. left. exact H. }
    rewrite firstn_skipn in Hfull. exact Hfull. }
  destruct Hin as [Heq | Hin].
  - left. symmetry. exact Heq.
  - right. split.
    + unfold remove_key in Hin. apply filter_In in Hin. tauto.
    + apply (removes_old_value k cs e). exact Hin.
Qed.

Print Assumptions capacity_enforced.
Print Assumptions oldest_eviction_length.
Print Assumptions new_entry_is_most_recent.
Print Assumptions retained_entries_have_provenance.
