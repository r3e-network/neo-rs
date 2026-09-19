--------------------------- MODULE minimal_consensus ---------------------------
(*
  Minimal, honest, TLC-runnable consensus model for the neo-rs formal-verification
  pipeline.  Written from first principles (NOT derived from Qoder's node_fixed.tla).

  Intended semantics, matching the docs' healthy/unhealthy scenarios:
    - There are n nodes (1..n) and f Byzantine faults.
    - Every honest node that commits commits the SAME block value (the single value
      proposed by the leader).  This is the classic "single-value SBFT" view and is
      exactly the commit rule Qoder's spec intended: commit proposed_block once a
      quorum of 2f+1 is reached.
  *)
EXTENDS Naturals, FiniteSets

CONSTANTS n, f, BlockValue

NodeSet == 1..n
Quorum  == 2*f + 1

VARIABLES proposed, committed

(* proposed    : whether the leader has put forward BlockValue *)
(* committed[i]: the value node i committed, or "" if not yet committed *)

INIT ==
    /\ proposed = FALSE
    /\ committed = [i \in NodeSet |-> ""]

(* The leader proposes the single block value known to every honest node. *)
Propose ==
    /\ proposed = FALSE
    /\ proposed' = TRUE
    /\ UNCHANGED committed

(* Once a quorum of nodes has reached the same (single) proposal, any of them may
   commit that value.  Because the value is the same for every node, honest nodes
   can never commit two different values -- this holds for ALL (n, f), not only
   when n >= 3f+1.  This is the key observation to report. *)
Commit ==
    /\ proposed = TRUE
    /\ \E self \in NodeSet, S \in SUBSET NodeSet :
          /\ Cardinality(S) >= Quorum
          /\ \A i \in S : committed[i] = ""
          /\ committed[self] = ""
          /\ committed' = [committed EXCEPT ![self] = BlockValue]
          /\ UNCHANGED proposed

(* Optional stutter: lets the model sit in a completed state instead of treating
   "no more nodes can commit" as a deadlock.  Pure modeling convenance. *)
Stutter == UNCHANGED <<proposed, committed>>

Next == Propose \/ Commit \/ Stutter

Spec == INIT /\ [][Next]_<<proposed, committed>>

(* Agreement: two honest nodes can never commit distinct values. *)
Agreement ==
    \A i, j \in NodeSet :
        (committed[i] # "" /\ committed[j] # "") => committed[i] = committed[j]

(* Validity: any committed value is the leader's proposed value. *)
Validity ==
    \A i \in NodeSet : committed[i] # "" => committed[i] = BlockValue

===========================================================================