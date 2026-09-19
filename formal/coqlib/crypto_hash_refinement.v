Require Import Arith Lia List.
Import ListNotations.

(**
  Cryptographic hash refinement - honest abstract model (Coq 8.18).

  The real SHA-256 / RIPEMD-160 primitives (neo-crypto/src/hash.rs) are
  treated as *explicit Section parameters* (`sha256`, `ripemd160`), together
  with their output-size contracts.  We prove the compositional facts that
  genuinely follow:

    - hash160 = RIPEMD160 o SHA256 has a 20-byte output;
    - hash256 = SHA256 o SHA256 has a 32-byte output;
    - the composition identities themselves;
    - determinism (both are pure total functions);
    - a constant-time equality contract, stated as an abstract parameter.

  We deliberately do NOT state or prove any collision-resistance / pre-image
  claim: those are not consequences of the abstract contracts and cannot be
  established without a concrete, audited SHA-256 implementation.  This model
  is an abstract contract; it does not refine the Rust executable byte-for-byte.
*)

Definition bytes := list nat.

Section CryptoHashRefinement.

Variable sha256 : bytes -> bytes.
Variable ripemd160 : bytes -> bytes.

(** Output-size contracts (explicit parameters, not machine-checkable here). *)
Hypothesis sha256_output_size : forall data : bytes, length (sha256 data) = 32.
Hypothesis ripemd160_output_size : forall data : bytes, length (ripemd160 data) = 20.

(** Hash160 = RIPEMD160(SHA256(data)) - matches neo-crypto/src/hash.rs. *)
Definition hash160 (data : bytes) : bytes := ripemd160 (sha256 data).

(** Hash256 = SHA256(SHA256(data)) - matches neo-crypto/src/hash.rs. *)
Definition hash256 (data : bytes) : bytes := sha256 (sha256 data).

(** ---- Output-size theorems ---- *)

Theorem hash160_output_size :
  forall data : bytes, length (hash160 data) = 20.
Proof.
  intros data. unfold hash160.
  rewrite (ripemd160_output_size (sha256 data)). reflexivity.
Qed.

Theorem hash256_output_size :
  forall data : bytes, length (hash256 data) = 32.
Proof.
  intros data. unfold hash256.
  rewrite (sha256_output_size (sha256 data)). reflexivity.
Qed.

(** ---- Compositional identities (by definition) ---- *)

Theorem hash160_composition_correctness :
  forall data : bytes, hash160 data = ripemd160 (sha256 data).
Proof. intros data. reflexivity. Qed.

Theorem hash256_composition_correctness :
  forall data : bytes, hash256 data = sha256 (sha256 data).
Proof. intros data. reflexivity. Qed.

(** ---- Determinism (pure total functions) ---- *)

Theorem sha256_deterministic :
  forall input1 input2 : bytes, input1 = input2 -> sha256 input1 = sha256 input2.
Proof. intros a b H. subst. reflexivity. Qed.

Theorem ripemd160_deterministic :
  forall input1 input2 : bytes, input1 = input2 -> ripemd160 input1 = ripemd160 input2.
Proof. intros a b H. subst. reflexivity. Qed.

Theorem hash256_deterministic :
  forall a b : bytes, a = b -> hash256 a = hash256 b.
Proof. intros a b H. subst. reflexivity. Qed.

(** Double SHA-256 still has a 32-byte output. *)
Lemma double_sha256_output_size : forall data : bytes, length (hash256 (hash256 data)) = 32.
Proof. intros data. apply hash256_output_size. Qed.

(** ---- Constant-time equality contract ---- *)

(** Mirrors `subtle::ConstantTimeEq` used in neo-crypto/src/hash.rs. *)
Variable ct_hash_eq : bytes -> bytes -> bool.

Hypothesis ct_hash_eq_correctness :
  forall a b : bytes,
    length a = length b -> (ct_hash_eq a b = true <-> a = b).

Hypothesis ct_hash_eq_different_lengths :
  forall a b : bytes, length a <> length b -> ct_hash_eq a b = false.

(** Constant-time equality is reflexive on equal-length buffers. *)
Lemma ct_eq_reflexive : forall a : bytes, ct_hash_eq a a = true.
Proof.
  intros a.
  apply (proj2 (ct_hash_eq_correctness a a eq_refl)). reflexivity.
Qed.

(** Equal-length buffers that compare true under ct_eq are equal. *)
Lemma ct_eq_true_implies_equal :
  forall a b : bytes, length a = length b -> ct_hash_eq a b = true -> a = b.
Proof.
  intros a b Hlen Heq.
  apply (proj1 (ct_hash_eq_correctness a b Hlen)). exact Heq.
Qed.

End CryptoHashRefinement.
