----------------------- MODULE neo_dbft_complete -----------------------
(* Single-height, single-view quorum abstraction, NOT the complete Neo protocol.
   Honest votes are globally visible; up to f Byzantine validators may support
   every value in every phase. This maximally-equivocating support abstraction
   removes message delivery permutations. It checks safety, not liveness,
   networking, signatures, block validation, recovery, or cross-view locking.
   M is explicit: production M = n - ((n-1) \div 3), not necessarily n-f
   when f is an injected fault count beyond the supported bound.
*)
EXTENDS Naturals, FiniteSets
CONSTANTS n, f, M, Values, None
ASSUME /\ n > 0 /\ f < n /\ M > 0 /\ M <= n
       /\ IsFiniteSet(Values) /\ Cardinality(Values) > 1 /\ None \notin Values

Validators == 1..n
Byzantine == 1..f
Honest == Validators \ Byzantine
Primary == 1

VARIABLES proposals, prepareVote, commitVote, decided
vars == <<proposals, prepareVote, commitVote, decided>>

PrepareSupport(v) == {i \in Honest : prepareVote[i] = v} \union Byzantine
CommitSupport(v) == {i \in Honest : commitVote[i] = v} \union Byzantine
Quorums == {q \in SUBSET Validators : Cardinality(q) = M}

Init ==
    /\ proposals = {}
    /\ prepareVote = [i \in Honest |-> None]
    /\ commitVote = [i \in Honest |-> None]
    /\ decided = [i \in Honest |-> None]

Propose(v) ==
    /\ v \notin proposals
    /\ Primary \in Byzantine \/ proposals = {}
    /\ proposals' = proposals \union {v}
    /\ UNCHANGED <<prepareVote, commitVote, decided>>

Prepare(i, v) ==
    /\ v \in proposals
    /\ prepareVote[i] = None
    /\ prepareVote' = [prepareVote EXCEPT ![i] = v]
    /\ UNCHANGED <<proposals, commitVote, decided>>

SignCommit(i, v) ==
    /\ prepareVote[i] = v
    /\ commitVote[i] = None
    /\ Cardinality(PrepareSupport(v)) >= M
    /\ commitVote' = [commitVote EXCEPT ![i] = v]
    /\ UNCHANGED <<proposals, prepareVote, decided>>

Finalize(i, v) ==
    /\ commitVote[i] = v
    /\ decided[i] = None
    /\ Cardinality(CommitSupport(v)) >= M
    /\ decided' = [decided EXCEPT ![i] = v]
    /\ UNCHANGED <<proposals, prepareVote, commitVote>>

Next ==
    \/ \E v \in Values : Propose(v)
    \/ \E i \in Honest, v \in Values : Prepare(i, v)
    \/ \E i \in Honest, v \in Values : SignCommit(i, v)
    \/ \E i \in Honest, v \in Values : Finalize(i, v)

Spec == Init /\ [][Next]_vars
TypeOK ==
    /\ proposals \subseteq Values
    /\ prepareVote \in [Honest -> Values \union {None}]
    /\ commitVote \in [Honest -> Values \union {None}]
    /\ decided \in [Honest -> Values \union {None}]

InvariantAgreement ==
    \A i, j \in Honest :
        (decided[i] # None /\ decided[j] # None) => decided[i] = decided[j]
InvariantValidity ==
    \A i \in Honest : decided[i] # None => decided[i] \in proposals
InvariantPreparedRequiresQuorum ==
    \A i \in Honest : commitVote[i] # None =>
        Cardinality(PrepareSupport(commitVote[i])) >= M
InvariantCommitRequiresQuorum ==
    \A i \in Honest : decided[i] # None =>
        Cardinality(CommitSupport(decided[i])) >= M
InvariantPhaseConsistency ==
    \A i \in Honest :
        /\ (commitVote[i] # None => commitVote[i] = prepareVote[i])
        /\ (decided[i] # None => decided[i] = commitVote[i])
InvariantQuorumIntersection ==
    \A a, b \in Quorums : (a \intersect b \intersect Honest) # {}

KeepVotes ==
    \A i \in Honest :
        /\ (prepareVote[i] # None => prepareVote'[i] = prepareVote[i])
        /\ (commitVote[i] # None => commitVote'[i] = commitVote[i])
KeepFinality ==
    \A i \in Honest : decided[i] # None => decided'[i] = decided[i]
InvariantNoEquivocation == [][KeepVotes]_vars
InvariantFinality == [][KeepFinality]_vars

(* Expected to FAIL in a separate witness run: not a production safety gate. *)
NoFinalization == \A i \in Honest : decided[i] = None
=======================================================================
