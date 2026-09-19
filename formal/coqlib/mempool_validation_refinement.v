Require Import Arith Lia List.
Import ListNotations.

(**
  Mempool validation - honest abstract model (Coq 8.18).

  Models the Neo N3 memory pool (neo-core/src/ledger/memory_pool) as a
  finite multiset-free list of transaction hashes plus a capacity, and the
  fee-priority ordering of `PoolItem::compare_to_transaction`.

  What is genuinely modeled and proved:
    - capacity is never exceeded after add + enforce (a REAL theorem, obtained
      from `firstn` trimming, in contrast to the vacuous claim in the prior
      version);
    - duplicate transactions are not re-added and do not grow the count;
    - removing a confirmed transaction removes it (it is no longer present);
    - the pool retains distinct hashes (a NoDup invariant);
    - fee priority ordering follows the Rust comparator structure:
      high-priority flag > fee-per-byte > network fee > (descending) hash.

  The prior file used a ghost `Map` and stated `mempool_never_exceeds_capacity`
  with the premise unrelated to the conclusion (vacuous: `size_limit_enforced
  initial -> size_limit_enforced final` holds for ANY two states).  Here the
  capacity statement is about an actual trimming function and is non-vacuous.

  This is an abstract model; it does not refine the Rust executable.
*)

(** Abstract transaction identity. *)
Definition tx_hash := nat.

(** Transaction fields relevant to mempool ordering. *)
Record Transaction := {
  t_high   : bool;   (* high-priority attribute present *)
  t_netfee : nat;    (* network fee, non-negative *)
  t_size   : nat;    (* script size in bytes, for fee-per-byte *)
  t_hash   : nat;    (* hash used as the final (descending) tie-breaker *)
}.

(** fee_per_byte matches the Rust helper: network_fee / script_size, 0 if empty. *)
Definition fee_per_byte (t : Transaction) : nat :=
  if Nat.eqb (t_size t) 0 then 0 else t_netfee t / t_size t.

(** Priority ordering per PoolItem::compare_to_transaction:
    high flag, then fee/byte, then network fee, then descending hash. *)
Definition better (a b : Transaction) : Prop :=
  (t_high a = true /\ t_high b = false) \/
  (t_high a = t_high b /\ fee_per_byte a > fee_per_byte b) \/
  (t_high a = t_high b /\ fee_per_byte a = fee_per_byte b
                /\ t_netfee a > t_netfee b) \/
  (t_high a = t_high b /\ fee_per_byte a = fee_per_byte b
                /\ t_netfee a = t_netfee b /\ t_hash a > t_hash b).

Lemma high_priority_wins :
  forall a b : Transaction, t_high a = true -> t_high b = false -> better a b.
Proof. intros a b Ha Hb. unfold better. auto. Qed.

Lemma fee_priority_correct :
  forall a b : Transaction, t_high a = t_high b -> fee_per_byte a > fee_per_byte b ->
    better a b.
Proof. intros a b Hh Hf. unfold better. right. left. auto. Qed.

Lemma network_fee_priority :
  forall a b : Transaction, t_high a = t_high b -> fee_per_byte a = fee_per_byte b ->
    t_netfee a > t_netfee b -> better a b.
Proof. intros a b Hh Hf Hn. unfold better. right. right. left. auto. Qed.

Lemma hash_priority :
  forall a b : Transaction,
    t_high a = t_high b -> fee_per_byte a = fee_per_byte b ->
    t_netfee a = t_netfee b -> t_hash a > t_hash b -> better a b.
Proof. intros a b Hh Hf Hn Hg. unfold better. right. right. right. auto. Qed.

Lemma fee_per_byte_nonneg : forall t : Transaction, 0 <= fee_per_byte t.
Proof. intros t. unfold fee_per_byte. destruct (Nat.eqb (t_size t) 0); lia. Qed.

(** ---- Capacity enforcement ---- *)

Definition pool_count (pool : list tx_hash) : nat := length pool.

(** Trimming to capacity: keep at most `cap` elements (Rust removes over capacity). *)
Definition enforce_capacity (pool : list tx_hash) (cap : nat) : list tx_hash :=
  firstn cap pool.

(** THE non-vacuous capacity statement: after trimming, the count never exceeds
    the capacity. *)
Theorem mempool_never_exceeds_capacity :
  forall (pool : list tx_hash) (cap : nat),
    length (enforce_capacity pool cap) <= cap.
Proof.
  intros pool cap. apply firstn_le_length.
Qed.

(** Add a transaction then enforce capacity. *)
Definition add_transaction (pool : list tx_hash) (cap tx : nat) : list tx_hash :=
  enforce_capacity (pool ++ [tx]) cap.

Lemma add_then_enforce_respects_capacity :
  forall (pool : list tx_hash) (cap tx : nat),
    length (add_transaction pool cap tx) <= cap.
Proof.
  intros pool cap tx. apply firstn_le_length.
Qed.

(** ---- Duplicate rejection ---- *)

Definition mem_tx (tx : tx_hash) (pool : list tx_hash) : bool :=
  existsb (Nat.eqb tx) pool.

(** Add only if not already present. *)
Definition add_no_duplicate (pool : list tx_hash) (tx : tx_hash) : list tx_hash :=
  if mem_tx tx pool then pool else pool ++ [tx].

Lemma mem_tx_true_iff_in : forall tx pool,
  mem_tx tx pool = true <-> In tx pool.
Proof.
  intros tx pool. unfold mem_tx.
  rewrite existsb_exists.
  split.
  - intros [x [Hin Heq]]. apply Nat.eqb_eq in Heq. subst. exact Hin.
  - intros Hin. exists tx. split; [exact Hin | apply Nat.eqb_refl].
Qed.

Lemma add_no_duplicate_preserves_when_present :
  forall pool tx, In tx pool -> add_no_duplicate pool tx = pool.
Proof.
  intros pool tx Hin.
  unfold add_no_duplicate, mem_tx.
  assert (Hm : existsb (Nat.eqb tx) pool = true).
  { apply existsb_exists. exists tx. split; [exact Hin | apply Nat.eqb_refl]. }
  rewrite Hm. reflexivity.
Qed.

Lemma duplicate_does_not_grow_count :
  forall pool tx, In tx pool -> length (add_no_duplicate pool tx) = length pool.
Proof.
  intros pool tx Hin.
  rewrite add_no_duplicate_preserves_when_present; auto.
Qed.

(** Adding a fresh transaction grows the count by exactly one (before trimming). *)
Lemma add_fresh_grows_count :
  forall (pool : list tx_hash) (tx : tx_hash), ~ In tx pool -> length (pool ++ [tx]) = length pool + 1.
Proof.
  intros pool tx Hnot. rewrite app_length. simpl. lia.
Qed.

(** ---- Confirmed transaction removal ---- *)

Definition remove_transaction (pool : list tx_hash) (tx : tx_hash) : list tx_hash :=
  filter (fun x => negb (Nat.eqb x tx)) pool.

Lemma removed_tx_not_present :
  forall pool tx, ~ In tx (remove_transaction pool tx).
Proof.
  intros pool tx Hin.
  unfold remove_transaction in *.
  apply filter_In in Hin. destruct Hin as [_ Hp].
  simpl in Hp. rewrite Nat.eqb_refl in Hp. discriminate.
Qed.

(** ---- Distinct-hash invariant ---- *)

Lemma add_preserves_no_dup :
  forall (pool : list tx_hash) (tx : tx_hash), NoDup pool -> ~ In tx pool -> NoDup (pool ++ [tx]).
Proof.
  intros pool.
  induction pool as [|h t IH]; intros tx Hnd Hnot.
  - simpl. constructor.
    + intro Hx. inversion Hx.
    + constructor.
  - simpl. constructor.
    + intro Hh.
      apply in_app_iff in Hh. destruct Hh as [Hh_t | Hh_cons].
      * inversion Hnd. contradiction.
      * destruct Hh_cons as [Heq | Hrest].
        -- subst. apply Hnot. simpl. auto.
        -- inversion Hrest.
    + apply IH.
      * inversion Hnd. auto.
      * intro Htx_t. apply Hnot. simpl. right. exact Htx_t.
Qed.

Theorem add_no_duplicate_preserves_no_dup :
  forall (pool : list tx_hash) (tx : tx_hash), NoDup pool -> NoDup (add_no_duplicate pool tx).
Proof.
  intros pool tx Hnd.
  destruct (mem_tx tx pool) eqn:E.
  - apply mem_tx_true_iff_in in E.
    rewrite (add_no_duplicate_preserves_when_present pool tx E). exact Hnd.
  - assert (Hnot : ~ In tx pool).
    { intro Hin. apply mem_tx_true_iff_in in Hin. rewrite Hin in E. discriminate. }
    unfold add_no_duplicate. rewrite E. apply add_preserves_no_dup; auto.
Qed.
