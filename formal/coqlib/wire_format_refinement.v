(**
  Coq refinement (Phase-1): Neo consensus message wire format length.

  Rust source mirrored:
    neo-consensus/src/messages/mod.rs -> ConsensusPayload::to_message_bytes
      wire layout: [type:1][block_index:4le][validator_index:1][view_number:1][body...]
      => length = 7 + body.len()
    (round-trip already proptestted in neo-consensus/tests/consensus_properties.rs)

  Proves the header length is constant 7 and that the total wire length is
  monotone in the body length.  A differential Rust test ties this to the live
  `to_message_bytes` length.

  Honest scoping: spec-level refinement start for serialization; executable gap
  per RFC.
*)
From Coq Require Import Arith Lia.
Open Scope nat_scope.

(* Fixed header: [type := 1][block_index := 4][validator_index := 1][view := 1] = 7 bytes. *)
Definition HEADER_BYTES : nat := 7.

(* Total wire length for a given body length. *)
Definition wire_len (body_len : nat) : nat := HEADER_BYTES + body_len.

(* The wire length is exactly header + body. *)
Lemma wire_len_header_plus_body :
  forall b, wire_len b = HEADER_BYTES + b.
Proof. intros b; unfold wire_len; auto. Qed.

(* A header is never empty: total wire length is always at least 7. *)
Lemma wire_len_at_least_header :
  forall b, 7 <= wire_len b.
Proof. intros b; unfold wire_len; apply Nat.le_add_r. Qed.

(* Monotone: a longer body yields a longer wire message (no truncation). *)
Lemma wire_len_monotone :
  forall a b, a <= b -> wire_len a <= wire_len b.
Proof. intros a b H; unfold wire_len; apply Nat.add_le_mono_l; auto. Qed.

(* Equal bodies yield equal wire lengths (deterministic header size). *)
Lemma wire_len_equal_if_body_equal :
  forall a b, a = b -> wire_len a = wire_len b.
Proof. intros a b H; subst; auto. Qed.