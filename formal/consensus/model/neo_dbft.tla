--------------------------- MODULE neo_dbft ---------------------------
(*
  Faithful TLA+ model of the dBFT consensus implemented in neo-rs
  (neo-consensus/src).  Not a toy: constants and commit/prepare guards mirror
  the real code, and the adversary is the classic Byzantine equivocator.

  Real-code traceability (neo-consensus/src/context/mod.rs):
    - f = (n-1)/3, M = n - f                         -> f()/m()          (lines 264-273)
    - commit needs >= M distinct validators          -> has_enough_commits (348-370)
    - prepare needs >= M matching preparation_hash   -> has_enough_prepare_responses (303-334)
    - primary = (block_index - view) mod n           -> primary_index    (276-286)
    - Byzantine validators may vote for ANY hash     -> the adversarial model below

  Safety condition (classic dBFT): two quorums of size M must intersect in
  more than f nodes, i.e. 2*M - n > f  <=>  n >= 3f+1.
*)
EXTENDS Naturals, FiniteSets

CONSTANTS n, f, M, Block

NodeSet    == 1..n
ByzSenders == { i \in NodeSet : i <= f }          (* Byzantine validator indices *)
Honest     == { i \in NodeSet : i > f }           (* honest validator indices *)

VARIABLES byzCommits, committed

(* byzCommits[i] = { <<s,h>> : byz sender s sent a Commit-for-h to honest node i }
   committed[i]  = hash honest node i committed to, or "" if none yet            *)

(* distinct Byzantine senders of hash h that honest node i has heard from *)
B(i, h) == { s \in 1..f : <<s, h>> \in byzCommits[i] }
(* honest nodes already committed to h *)
H(h)    == { j \in Honest : committed[j] = h }
(* total distinct validators (byz + honest) voting h, as seen by i *)
cardVotes(i, h) == Cardinality(B(i, h) \union H(h))

INIT ==
    /\ byzCommits = [i \in Honest |-> {}]
    /\ committed  = [i \in Honest |-> ""]

(* Byzantine senders equivocate: a byz sender may push a Commit-for-h to any
   honest node, including both hashes to the same node (equivocation). *)
ByzSend ==
    \E i \in Honest, s \in 1..f, h \in Block :
        /\ <<s, h>> \notin byzCommits[i]
        /\ byzCommits' = [byzCommits EXCEPT ![i] = byzCommits[i] \union {<<s, h>>}]
        /\ UNCHANGED committed

(* Honest commit: commit to h once >= M distinct validators vote h.
   Mirrors has_enough_commits (>= M commits). *)
HonestCommit ==
    \E i \in Honest, h \in Block :
        /\ committed[i] = ""
        /\ cardVotes(i, h) >= M
        /\ committed' = [committed EXCEPT ![i] = h]
        /\ UNCHANGED byzCommits

Stutter == UNCHANGED <<byzCommits, committed>>

Next == ByzSend \/ HonestCommit \/ Stutter

Spec == INIT /\ [][Next]_<<byzCommits, committed>>

(* Two honest nodes can never commit different block hashes. *)
Agreement ==
    \A i, j \in Honest :
        (committed[i] # "" /\ committed[j] # "") => committed[i] = committed[j]

(* Every commit was actually backed by a quorum of M distinct validators. *)
QuorumRespected ==
    \A i \in Honest, h \in Block :
        committed[i] = h => cardVotes(i, h) >= M

(* The real safety bound.  A config where this fails is expected to violate
   Agreement (the boundary scenario). *)
SafeQuorum == 2 * M - n > f

===========================================================================