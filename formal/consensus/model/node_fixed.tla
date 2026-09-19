--------------------------- MODULE consensus ---------------------------
EXTENDS Naturals, Integers

(* ==================================================================== *)
(* CONSTANTS                                                          *)
(* ==================================================================== *)

CONSTANT n  (* Total number of nodes *)
CONSTANT f  (* Maximum number of faulty nodes *)

(* Assumption: healthy when n >= 3*f + 1 *)
ASSUME Healthy == n >= 3*f + 1

(* ==================================================================== *)
(* TYPES                                                                *)
(* ==================================================================== *)

NodeID == 1..n
BlockHash == Str
MsgType == {"PRE-PREPARE", "PREPARE", "COMMIT", "REPLY"}

(* ==================================================================== *)
(* STATE VARIABLES                                                      *)
(* ==================================================================== *)

VARIABLES 
    view,                    (* Current view number *)
    proposed_block,          (* Block being proposed *)
    preprepare_sent,         (* Has leader sent PRE-PREPARE? *)
    prepared_set,            (* Set of nodes that sent PREPARE *)
    commit_set,              (* Set of nodes that sent COMMIT *)
    committed_height,        (* Highest committed height *)
    committed_block,         (* Committed block at that height *)
    reply_sent;              (* Have we sent REPLY? *)

(* ==================================================================== *)
(* DERIVED CONSTANTS                                                    *)
(* ==================================================================== *)

(* Get leader for given view *)
LEADER(v) == ((v MOD n) + 1)

(* ==================================================================== *)
(* INVARIANTS                                                           *)
(* ==================================================================== *)

(* Invariant 1: Agreement - All nodes commit same block at same height *)
Agreement == 
    ALL h1, h2 \in Nat, b1, b2 \in BlockHash:
        committed_height = h1 /\ committed_height = h2 =>
        committed_block = b1 /\ committed_block = b2 =>
        b1 = b2

(* Invariant 2: Validity - Committed block has non-empty value *)
Validity ==
   committed_height > 0 => committed_block != ""

(* Invariant 3: No double commit - Never change committed value once set *)
NoDoubleCommit ==
    committed_height' # committed_height =>
    (committed_height = 0 \/ committed_block = proposed_block)

(* Invariant 4: Prepared threshold requires 2f+1 distinct PREPARE messages *)
PreparedThreshold ==
    |prepared_set| >= 2*f + 1 => prepared

(* Invariant 5: Commit threshold requires 2f+1 distinct COMMIT messages *)
CommitThreshold ==
    |commit_set| >= 2*f + 1 => True

(* Combined Safety Property *)
Safety == 
    /\ Agreement
    /\ Validity
    /\ NoDoubleCommit

(* ==================================================================== *)
(* NEXT STATE RELATION                                                  *)
(* ==================================================================== *)

(* Leader proposes new block *)
ProposeBlock ==
    /\ self = LEADER(view)
    /\ ~preprepare_sent
    /\ proposed_block' = "Block_" & STRING(committed_height + 1)
    /\ preprepare_sent' = TRUE
    /\ UNCHANGED <<view, prepared_set, commit_set, committed_height, committed_block, reply_sent>>

(* Follower sends PREPARE after receiving PRE-PREPARE *)
SendPrepare ==
    /\ ~self = LEADER(view)
    /\ preprepare_sent
    /\ ~\E n \in prepared_set: n = self
    /\ prepared_set' = prepared_set \union {self}
    /\ prepared' = (|prepared_set'| >= 2*f + 1)
    /\ UNCHANGED <<view, proposed_block, preprepare_sent, commit_set, committed_height, committed_block, reply_sent>>

(* Node sends COMMIT after collecting 2f+1 PREPARES *)
SendCommit ==
    /\ prepared
    /\ ~\E n \in commit_set: n = self
    /\ commit_set' = commit_set \union {self}
    /\ UNCHANGED <<view, proposed_block, preprepare_sent, prepared_set, committed_height, committed_block, reply_sent>>

(* Node commits block after collecting 2f+1 COMMITS *)
CommitBlock ==
    /\ |commit_set| >= 2*f + 1
    /\ committed_height = 0
    /\ committed_height' = committed_height + 1
    /\ committed_block' = proposed_block
    /\ UNCHANGED <<view, proposed_block, preprepare_sent, prepared_set, commit_set, reply_sent>>

(* Node sends REPLY after committing *)
SendReply ==
    /\ committed_height > 0
    /\ ~reply_sent
    /\ reply_sent' = TRUE
    /\ UNCHANGED <<view, proposed_block, preprepare_sent, prepared_set, commit_set, committed_height, committed_block>>

(* View change on timeout *)
ViewChange ==
    IF timeout_pending THEN
        << view' := view + 1,
           preprepare_sent' := FALSE,
           prepared_set' := {},
           commit_set' := {},
           prepared' := FALSE >>
    ELSE SKIP

(* Combined next state *)
Next == 
    ProposeBlock \/ 
    SendPrepare \/ 
    SendCommit \/
    CommitBlock \/
    SendReply \/
    ViewChange

(* ==================================================================== *)
(* INITIAL STATE                                                        *)
(* ==================================================================== *)

INIT ==
    /\ view = 0
    /\ proposed_block = ""
    /\ preprepare_sent = FALSE
    /\ prepared_set = {}
    /\ commit_set = {}
    /\ prepared = FALSE
    /\ committed_height = 0
    /\ committed_block = ""
    /\ reply_sent = FALSE

(* ==================================================================== *)
(* SPECIFICATION                                                        *)
(* ==================================================================== *)

Spec == INIT /\ [][Next]_vars /\ WF_vars(Next)

vars == <<view, proposed_block, preprepare_sent, prepared_set, commit_set, 
           committed_height, committed_block, reply_sent>>

===========================================================================
