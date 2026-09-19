(**
  Consensus Message Pool Round-Trip Refinement — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, abstract; NOT a mechanical refinement):
    neo-consensus/src/context/mod.rs pools that store per-validator messages
    (prepare_responses, commits), indexed by validator index.

  The old file used non-existent imports (Maybe, byte, block, is_honest_validator)
  and asserted essentially tautological/fake "liveness" theorems.  This honest
  model keeps only the machine-checkable content: a message stored into a
  per-validator pool can be retrieved unchanged (pool round-trip identity), and
  storing under one validator does not disturb another validator's entry
  (non-interference).  Machine-checked:
    - store then retrieve returns the same message (prepare pool),
    - store then retrieve returns the same message (commit pool),
    - non-interference: updating validator v2 leaves v1's entry intact when
      v1 <> v2.

  Boundaries:
    - No claim about dBFT liveness, finality, or strategy-level safety.  Those
      require a much larger state machine and are NOT asserted here.
    - Pools are abstract functions nat -> option consensus_msg, not a
      refinement of the concrete Rust map data structure.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

(* Consensus message kinds (Neo dBFT). *)
Inductive msg_type : Type :=
  | PrepareRequest
  | PrepareResponse
  | Commit
  | ChangeView
  | RecoveryRequest
  | RecoveryMsg.

(* Abstract consensus message.  hashes/reasons are opaque nat placeholders. *)
Record consensus_msg := make_msg {
  m_type          : msg_type;
  m_block_index   : nat;
  m_validator     : nat;
  m_view          : nat;
  m_hash          : option nat;
  m_timestamp     : nat;
  m_signature     : list nat;
  m_reason        : option nat
}.

(* Per-validator message pools for prepare responses and commits. *)
Record consensus_state := make_state {
  prepare_responses : nat -> option consensus_msg;
  commits           : nat -> option consensus_msg
}.

(* Store a message into the prepare pool of validator v. *)
Definition store_prepare (s : consensus_state) (v : nat) (m : consensus_msg) : consensus_state :=
  {| prepare_responses := (fun i => if Nat.eqb i v then Some m else s.(prepare_responses) i);
     commits           := s.(commits) |}.

(* Store a message into the commit pool of validator v. *)
Definition store_commit (s : consensus_state) (v : nat) (m : consensus_msg) : consensus_state :=
  {| commits := (fun i => if Nat.eqb i v then Some m else s.(commits) i);
     prepare_responses := s.(prepare_responses) |}.

(* Round-trip identity: a prepare response stored and retrieved is unchanged. *)
Lemma message_pool_identity_prepare :
  forall (s : consensus_state) (v : nat) (m : consensus_msg),
    (store_prepare s v m).(prepare_responses) v = Some m.
Proof.
  intros s v m. unfold store_prepare. simpl. rewrite Nat.eqb_refl. reflexivity.
Qed.

(* Round-trip identity: a commit stored and retrieved is unchanged. *)
Lemma message_pool_identity_commit :
  forall (s : consensus_state) (v : nat) (m : consensus_msg),
    (store_commit s v m).(commits) v = Some m.
Proof.
  intros s v m. unfold store_commit. simpl. rewrite Nat.eqb_refl. reflexivity.
Qed.

(* Non-interference: storing for v2 leaves v1's prepare entry intact (v1 <> v2). *)
Lemma store_prepare_irrelevant :
  forall (s : consensus_state) (v1 v2 : nat) (m : consensus_msg),
    v1 <> v2 ->
    (store_prepare s v2 m).(prepare_responses) v1 = s.(prepare_responses) v1.
Proof.
  intros s v1 v2 m Hneq. unfold store_prepare. simpl.
  destruct (Nat.eqb v1 v2) eqn:E.
  - apply Nat.eqb_eq in E. contradiction.
  - reflexivity.
Qed.

(* Non-interference: storing for v2 leaves v1's commit entry intact (v1 <> v2). *)
Lemma store_commit_irrelevant :
  forall (s : consensus_state) (v1 v2 : nat) (m : consensus_msg),
    v1 <> v2 ->
    (store_commit s v2 m).(commits) v1 = s.(commits) v1.
Proof.
  intros s v1 v2 m Hneq. unfold store_commit. simpl.
  destruct (Nat.eqb v1 v2) eqn:E.
  - apply Nat.eqb_eq in E. contradiction.
  - reflexivity.
Qed.

(* Storing a message never changes the other pool. *)
Lemma store_prepare_keeps_commits :
  forall (s : consensus_state) (v : nat) (m : consensus_msg),
    (store_prepare s v m).(commits) = s.(commits).
Proof.
  intros s v m. unfold store_prepare. reflexivity.
Qed.

Lemma store_commit_keeps_prepares :
  forall (s : consensus_state) (v : nat) (m : consensus_msg),
    (store_commit s v m).(prepare_responses) = s.(prepare_responses).
Proof.
  intros s v m. unfold store_commit. reflexivity.
Qed.

(** Concrete round-trip example. *)
Example store_then_get_prepare_example :
  let s := make_state (fun _ => None) (fun _ => None) in
  let m := make_msg PrepareResponse 42 7 3 (Some 0) 1000 nil (Some 1) in
  (store_prepare s 7 m).(prepare_responses) 7 = Some m.
Proof.
  unfold store_prepare. simpl. reflexivity.
Qed.