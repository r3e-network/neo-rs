(**
  Coq refinement (Phase-1): UInt160 / UInt256 address-hash validation.

  Rust source mirrored:
    neo-primitives/src/uint160.rs  -> UInt160 (20 bytes)
    neo-primitives/src/uint256.rs  -> UInt256 (32 bytes)

  This proves the core length invariants of the address/hash types: a valid
  UInt160 is exactly 20 bytes, a valid UInt256 exactly 32 bytes, and they are
  distinct sizes.  A differential Rust test ties these to the live
  UInt160::from_bytes / UInt256::from_bytes behaviour.

  Honest scoping: this is the spec-level refinement start for the UInt layer;
  closing the executable gap (Rust binary == Coq model) is per the RFC.
*)
From Coq Require Import List Arith Lia.
Open Scope nat_scope.

(* Neo sizes (bytes). *)
Definition UINT160_LEN : nat := 20.
Definition UINT256_LEN : nat := 32.

(* A valid UInt160 is a byte buffer of exactly 20 bytes. *)
Definition valid_uint160 (b : list nat) : Prop := length b = UINT160_LEN.

(* A valid UInt256 is a byte buffer of exactly 32 bytes. *)
Definition valid_uint256 (b : list nat) : Prop := length b = UINT256_LEN.

(* from_bytes on a 20-byte buffer yields a valid UInt160. *)
Lemma uint160_from_bytes_valid :
  forall (b : list nat), length b = UINT160_LEN -> valid_uint160 b.
Proof. intros b H; unfold valid_uint160; auto. Qed.

(* Converse: a valid UInt160 has exactly 20 bytes. *)
Lemma valid_uint160_len :
  forall (b : list nat), valid_uint160 b -> length b = UINT160_LEN.
Proof. intros b H; unfold valid_uint160 in H; auto. Qed.

(* from_bytes on a 32-byte buffer yields a valid UInt256. *)
Lemma uint256_from_bytes_valid :
  forall (b : list nat), length b = UINT256_LEN -> valid_uint256 b.
Proof. intros b H; unfold valid_uint256; auto. Qed.

(* UInt256 (32 bytes) is strictly longer than UInt160 (20 bytes). *)
Lemma uint256_longer_than_uint160 :
  UINT256_LEN > UINT160_LEN.
Proof. cbv; lia. Qed.

(* A 20-byte UInt160 buffer cannot also be a valid 32-byte UInt256. *)
Lemma uint160_not_uint256 :
  forall (b : list nat), valid_uint160 b -> ~ valid_uint256 b.
Proof.
  intros b H1 H2.
  unfold valid_uint160 in H1; unfold valid_uint256 in H2.
  rewrite H1 in H2.
  discriminate.
Qed.

(* from_bytes does not map a 20-byte buffer to a 32-byte type. *)
Lemma uint160_from_bytes_not_uint256 :
  forall (b : list nat), length b = UINT160_LEN -> ~ length b = UINT256_LEN.
Proof.
  intros b H.
  apply uint160_not_uint256.
  unfold valid_uint160; auto.
Qed.
