----------------------------- MODULE neo_refcount ------------------------------
(*
  Faithful model of neo-rs ReferenceCounter safety (neo-vm/src/reference_counter.rs).

  The real GC (check_zero_referred) removes an item/component only when NO member
  has stack references and NO member has an external (outside-component) parent
  reference.  We abstract that to per-item:
    - sr[i]    : stack_references      (>= 0)
    - extP[i]  : external parent refs  (>= 0)  -- keeps an item alive
    - collected[i] : TRUE once the item has been reclaimed

  Safety invariants:
    - NoNegRefs            : counters never underflow
    - NoCollectWhileReachable : an item with sr>0 or an external parent is never
                                collected  (this is what the Tarjan keep-check enforces)

  The file ships two next-state operators:
    - Next       : correct protocol  -> invariants HOLD
    - BuggyNext  : a collector that reclaims reachable items -> NoCollectWhileReachable
                   is VIOLATED, demonstrating the invariant is not vacuous.
*)
EXTENDS Integers, FiniteSets

NumItems == 2

VARIABLES
  \* @type: Int -> Int;
  sr,
  \* @type: Int -> Int;
  extP,
  \* @type: Int -> Bool;
  collected

Init ==
  /\ sr = [i \in 1..NumItems |-> 0]
  /\ extP = [i \in 1..NumItems |-> 0]
  /\ collected = [i \in 1..NumItems |-> FALSE]

NoNegRefs ==
  \A i \in 1..NumItems : sr[i] >= 0 /\ extP[i] >= 0

NoCollectWhileReachable ==
  \A i \in 1..NumItems : collected[i] => (sr[i] = 0 /\ extP[i] = 0)

AddStack ==
  \E i \in 1..NumItems :
    /\ ~collected[i]
    /\ sr' = [sr EXCEPT ![i] = sr[i] + 1]
    /\ UNCHANGED <<extP, collected>>

RemoveStack ==
  \E i \in 1..NumItems :
    /\ ~collected[i] /\ sr[i] > 0
    /\ sr' = [sr EXCEPT ![i] = sr[i] - 1]
    /\ UNCHANGED <<extP, collected>>

AddParentRef ==
  \E i \in 1..NumItems :
    /\ ~collected[i]
    /\ extP' = [extP EXCEPT ![i] = extP[i] + 1]
    /\ UNCHANGED <<sr, collected>>

RemoveParentRef ==
  \E i \in 1..NumItems :
    /\ ~collected[i] /\ extP[i] > 0
    /\ extP' = [extP EXCEPT ![i] = extP[i] - 1]
    /\ UNCHANGED <<sr, collected>>

(* Correct collector: reclaim only when unreachable (sr = 0 and no external parent). *)
Collect ==
  \E i \in 1..NumItems :
    /\ ~collected[i]
    /\ sr[i] = 0 /\ extP[i] = 0
    /\ collected' = [collected EXCEPT ![i] = TRUE]
    /\ UNCHANGED <<sr, extP>>

Next == AddStack \/ RemoveStack \/ AddParentRef \/ RemoveParentRef \/ Collect

(* Deliberately buggy collector: reclaims items still holding references. *)
BuggyCollect ==
  \E i \in 1..NumItems :
    /\ ~collected[i]
    /\ (sr[i] > 0 \/ extP[i] > 0)
    /\ collected' = [collected EXCEPT ![i] = TRUE]
    /\ UNCHANGED <<sr, extP>>

BuggyNext == AddStack \/ RemoveStack \/ AddParentRef \/ RemoveParentRef \/ BuggyCollect

===============================================================================
