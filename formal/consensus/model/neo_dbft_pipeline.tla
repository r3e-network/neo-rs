--------------------------- MODULE neo_dbft_pipeline ---------------------------
(*
  Faithful two-phase dBFT pipeline for neo-rs: PrepareResponse accumulation,
  then Commit accumulation, gated by the real M-quorum guard.

  Real-code traceability (neo-consensus/src/context/mod.rs):
    - prepare needs >= M matching hashes   -> has_enough_prepare_responses (303-334)
    - commit only after verified proposal  -> can_sign_commit              (343-346)
    - commit needs >= M distinct validators-> has_enough_commits           (348-370)
    - honest broadcast of own vote         -> sending of own PrepareResponse/Commit

  Byzantine validators (Byz) may equivocate: they inject PrepareResponse and
  Commit messages for ANY hash to ANY honest node, including two different
  hashes to the same node.  Honest nodes (Honest) follow the protocol and
  never equivocate.
*)
EXTENDS Naturals, FiniteSets

CONSTANTS n, f, M, Block

NodeSet == 1..n
Byz     == { i \in NodeSet : i <= f }
Honest  == { i \in NodeSet : i > f }

VARIABLES prepMsg, commitMsg, prepared, committed

(* prepMsg[i]  = { <<sender, hash>> : PrepareResponse for hash received by honest node i }
   commitMsg[i] = { <<sender, hash>> : Commit for hash received by honest node i }
   prepared[i]  = hash honest node i has prepared ("" if none)
   committed[i] = hash honest node i has committed ("" if none)                     *)

PrepVotes(i, h) == { s \in NodeSet : <<s, h>> \in prepMsg[i] }
CommVotes(i, h) == { s \in NodeSet : <<s, h>> \in commitMsg[i] }
cardPrep(i, h)  == Cardinality(PrepVotes(i, h))
cardComm(i, h)  == Cardinality(CommVotes(i, h))

INIT ==
    /\ prepMsg   = [i \in Honest |-> {}]
    /\ commitMsg = [i \in Honest |-> {}]
    /\ prepared  = [i \in Honest |-> ""]
    /\ committed = [i \in Honest |-> ""]

(* Byzantine equivocation: inject a PrepareResponse for any hash to any honest node. *)
ByzPrepare ==
    \E i \in Honest, s \in Byz, h \in Block :
        /\ <<s, h>> \notin prepMsg[i]
        /\ prepMsg' = [prepMsg EXCEPT ![i] = prepMsg[i] \union {<<s, h>>}]
        /\ UNCHANGED <<commitMsg, prepared, committed>>

(* Byzantine equivocation: inject a Commit for any hash to any honest node. *)
ByzCommit ==
    \E i \in Honest, s \in Byz, h \in Block :
        /\ <<s, h>> \notin commitMsg[i]
        /\ commitMsg' = [commitMsg EXCEPT ![i] = commitMsg[i] \union {<<s, h>>}]
        /\ UNCHANGED <<prepMsg, prepared, committed>>

(* Honest prepare: reach M distinct PrepareResponse votes for h, then broadcast own vote. *)
Prepare ==
    \E i \in Honest, h \in Block :
        /\ prepared[i] = ""
        /\ cardPrep(i, h) >= M
        /\ prepared' = [prepared EXCEPT ![i] = h]
        /\ prepMsg'  = [j \in Honest |-> prepMsg[j] \union {<<i, h>>}]
        /\ UNCHANGED <<commitMsg, committed>>

(* Honest commit: only after having prepared h, reach M distinct Commit votes for h,
   then broadcast own Commit.  Mirrors can_sign_commit -> has_enough_commits. *)
Commit ==
    \E i \in Honest, h \in Block :
        /\ committed[i] = ""
        /\ prepared[i] = h
        /\ cardComm(i, h) >= M
        /\ committed' = [committed EXCEPT ![i] = h]
        /\ commitMsg' = [j \in Honest |-> commitMsg[j] \union {<<i, h>>}]
        /\ UNCHANGED <<prepMsg, prepared>>

Stutter == UNCHANGED <<prepMsg, commitMsg, prepared, committed>>

Next == ByzPrepare \/ ByzCommit \/ Prepare \/ Commit \/ Stutter

Spec == INIT /\ [][Next]_<<prepMsg, commitMsg, prepared, committed>>

(* Agreement: two honest nodes can never commit different block hashes. *)
Agreement ==
    \A i, j \in Honest :
        (committed[i] # "" /\ committed[j] # "") => committed[i] = committed[j]

(* Commit only ever happens after the same node prepared the same hash. *)
CommitRequiresPrepare ==
    \A i \in Honest, h \in Block : committed[i] = h => prepared[i] = h

(* Every commit was backed by a quorum of M distinct validators. *)
QuorumRespected ==
    \A i \in Honest, h \in Block : committed[i] = h => cardComm(i, h) >= M

(* The real safety bound (two quorums of size M must intersect in > f nodes). *)
SafeQuorum == 2 * M - n > f

===========================================================================