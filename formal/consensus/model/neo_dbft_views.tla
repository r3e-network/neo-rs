--------------------------- MODULE neo_dbft_views ---------------------------
(*
  Phase A: cross-view NoDoubleCommit / Agreement for the neo-rs dBFT.

  Abstraction note (sound for SAFETY, not liveness):
    Safety properties depend only on vote counts and honest commitment being
    sticky, not on message timing.  We therefore collapse Byzantine power to
    "up to f votes per (honest node, block hash)" and make honest commits
    sticky across all views.  This proves NoDoubleCommit / Agreement under the
    real bound 2M - n > f, and keeps n=7 tractable in plain TLC (no Apalache).

    Real-code mapping (neo-consensus/src/context/mod.rs):
      - commits are view-scoped, M needed        -> has_enough_commits (348-370)
      - a committed validator is sticky          -> ConsensusState::Committed / recovery
      - Byzantine validators may vote for any hash -> adversarial model

    Liveness / recovery timing is intentionally out of this model; it is a
    separate specification (documented in reports/formal/VERIFICATION-PLAN.md).
*)
EXTENDS Naturals, FiniteSets

CONSTANTS n, f, M, Block

NodeSet == 1..n
Byz     == { i \in NodeSet : i <= f }
Honest  == { i \in NodeSet : i > f }

VARIABLES byzVotes, honestCommit

(* byzVotes[i]     = hashes for which Byzantine backs honest node i (f votes each) *)
(* honestCommit[i] = hash honest node i committed to ("" if none; sticky)           *)

cardVotes(i, h) ==
    (IF h \in byzVotes[i] THEN f ELSE 0)
    + Cardinality({ j \in Honest : honestCommit[j] = h })

INIT ==
    /\ byzVotes    = [i \in Honest |-> {}]
    /\ honestCommit = [i \in Honest |-> ""]

(* Byzantine extends support: back hash h for honest node i (equivocation is free). *)
ByzAdd ==
    \E i \in Honest, h \in Block :
        /\ h \notin byzVotes[i]
        /\ byzVotes' = [byzVotes EXCEPT ![i] = byzVotes[i] \union {h}]
        /\ UNCHANGED honestCommit

(* Honest commit: reach M distinct validators (byz + honest) voting h.  Sticky:
   once committed, an honest node never votes for a different hash. *)
Commit ==
    \E i \in Honest, h \in Block :
        /\ honestCommit[i] = ""
        /\ cardVotes(i, h) >= M
        /\ honestCommit' = [honestCommit EXCEPT ![i] = h]
        /\ UNCHANGED byzVotes

Stutter == UNCHANGED <<byzVotes, honestCommit>>

Next == ByzAdd \/ Commit \/ Stutter

Spec == INIT /\ [][Next]_<<byzVotes, honestCommit>>

(* Agreement / NoDoubleCommit: no two honest nodes commit different hashes,
   across all views (a committed node never flips). *)
Agreement ==
    \A i, j \in Honest :
        (honestCommit[i] # "" /\ honestCommit[j] # "") => honestCommit[i] = honestCommit[j]

(* Every commit was backed by a quorum of M distinct validators. *)
QuorumRespected ==
    \A i \in Honest, h \in Block : honestCommit[i] = h => cardVotes(i, h) >= M

(* Real safety bound: two quorums of size M intersect in more than f nodes. *)
SafeQuorum == 2 * M - n > f

===========================================================================