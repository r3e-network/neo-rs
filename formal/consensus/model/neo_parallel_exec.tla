---------------------------- MODULE neo_parallel_exec ---------------------------
(*
  Parallel-execution safety for neo-rs optimistic execution
  (neo-vm/src/runtime/rw_set_tracker.rs).

  RwSetTracker::conflicts_with returns true when a transaction WRITES a key that
  another transaction READS or WRITES.  Two transactions may run in parallel only
  when their write sets are disjoint (write-write) and no write-read overlap.

  Model: two transactions over keys 1..NumKeys write to their own key sets.
  Invariant SerialEquivalence: after both finish, every key holds the value its
  writer wrote.  If write sets are DISJOINT this holds under ANY interleaving
  (parallel == serial, order-independent).

  This file ships two next-state operators:
    - Next         : disjoint write sets -> SerialEquivalence HOLDS (PASS)
    - ConflictNext : overlapping write sets (a write-write conflict) ->
                     SerialEquivalence is VIOLATED (order now matters)
*)
EXTENDS Integers, FiniteSets

NumKeys == 3
InitVal == 0

VARIABLES
  \* @type: Int -> Int;
  store,
  \* @type: Int -> Bool;
  performed

Init ==
  /\ store = [k \in 1..NumKeys |-> InitVal]
  /\ performed = [i \in 1..2 |-> FALSE]

(* Disjoint writes: T1 owns keys {1,2}, T2 owns key {3}. *)
T1Do ==
  /\ ~performed[1]
  /\ store' = [store EXCEPT ![1] = 1, ![2] = 1]
  /\ performed' = [performed EXCEPT ![1] = TRUE]

T2Do ==
  /\ ~performed[2]
  /\ store' = [store EXCEPT ![3] = 2]
  /\ performed' = [performed EXCEPT ![2] = TRUE]

(* Conflicting writes: both transactions write key {2} (write-write conflict). *)
T1Conflict ==
  /\ ~performed[1]
  /\ store' = [store EXCEPT ![1] = 1, ![2] = 1]
  /\ performed' = [performed EXCEPT ![1] = TRUE]

T2Conflict ==
  /\ ~performed[2]
  /\ store' = [store EXCEPT ![2] = 2, ![3] = 2]
  /\ performed' = [performed EXCEPT ![2] = TRUE]

Stutter == UNCHANGED <<store, performed>>

Next == T1Do \/ T2Do \/ Stutter
ConflictNext == T1Conflict \/ T2Conflict \/ Stutter

(* Parallel result equals serial at every reachable state: each key holds its
   writer's value iff that writer has performed (a proper inductive invariant).
   Holds for disjoint write sets; broken by a write-write conflict. *)
SerialEquivalence ==
  \A k \in 1..NumKeys :
    store[k] =
      IF k = 1 \/ k = 2 THEN (IF performed[1] THEN 1 ELSE InitVal)
      ELSE IF k = 3 THEN (IF performed[2] THEN 2 ELSE InitVal)
      ELSE InitVal

===============================================================================
