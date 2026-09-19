From Coq Require Import List Arith Lia.
Import ListNotations.

(** Witness script-hash computation aligned with neo-core/src/witness.rs
    `Witness::script_hash` (RIPEMD160(SHA256(verification_script))) and
    neo-core/src/smart_contract/helper.rs `signature_redeem_script` (Neo N3).
    SHA-256, RIPEMD-160 and the CheckSig syscall hash are explicit abstract
    Section parameters; no claim of collision resistance or of a byte-exact
    match to the concrete hashing.

    Honest correction vs the legacy model: the single-signature redeem script is
    the 40-byte N3 layout `PUSHDATA1(0x0C) 33 <33-byte key> SYSCALL(0x68)
    <4-byte CheckSig hash>`, NOT the 35-byte Neo2 layout `0x21 <key> 0xAC`.
    (The legacy `single_sig_pattern_length : length ([33+2:nat]) = 35` and the
    `35`-byte claims are accordingly corrected to 40.)  The legacy unproved
    `proof_empty_vs_nonempty` (a collision claim for different scripts) is
    dropped: hash preimage/collision properties are not asserted.
*)

Section WitnessScriptHash.

Definition bytes := list nat.
Definition public_key := bytes.

Definition UINT160_LENGTH : nat := 20.
Definition UINT256_LENGTH : nat := 32.

Variable sha256_abstract : bytes -> bytes.
Variable ripemd160_abstract : bytes -> bytes.
Hypothesis sha256_output_size : forall b, length (sha256_abstract b) = UINT256_LENGTH.
Hypothesis ripemd160_output_size : forall b, length (ripemd160_abstract b) = UINT160_LENGTH.

(** THEOREM: the script hash is always a UInt160 (20 bytes). *)
Definition compute_script_hash (script : bytes) : bytes :=
  ripemd160_abstract (sha256_abstract script).

Theorem script_hash_is_uint160 :
  forall script, length (compute_script_hash script) = UINT160_LENGTH.
Proof.
  intros script. unfold compute_script_hash. rewrite ripemd160_output_size. reflexivity.
Qed.

(** THEOREM: the script hash is deterministic (pure function). *)
Theorem script_hash_deterministic :
  forall s1 s2, s1 = s2 -> compute_script_hash s1 = compute_script_hash s2.
Proof.
  intros s1 s2 Heq. subst. reflexivity.
Qed.

(** ---- 40-byte N3 single-signature redeem script ---- *)

Variable check_sig_hash : bytes.
Hypothesis check_sig_hash_size : length check_sig_hash = 4.

Definition signature_redeem_script (pk : public_key) : bytes :=
  [0x0C; 33] ++ pk ++ [0x68] ++ check_sig_hash.

(** THEOREM: a 33-byte compressed public key yields a 40-byte redeem script
    (corrects the legacy 35-byte claim). *)
Theorem redeem_script_length_valid :
  forall pk, length pk = 33 -> length (signature_redeem_script pk) = 40.
Proof.
  intros pk Hlen.
  unfold signature_redeem_script.
  rewrite !app_length.
  simpl.
  rewrite Hlen, check_sig_hash_size.
  lia.
Qed.

(** THEOREM: the redeem script opens with PUSHDATA1 (0x0C) then the length byte 33. *)
Lemma redeem_script_opcodes :
  forall pk,
    nth 0 (signature_redeem_script pk) 0 = 0x0C /\
    nth 1 (signature_redeem_script pk) 0 = 33.
Proof.
  intros pk. split.
  - unfold signature_redeem_script. simpl. reflexivity.
  - unfold signature_redeem_script. simpl. reflexivity.
Qed.

(** THEOREM: position 35 is the SYSCALL opcode (0x68), matching helper.rs
    `is_signature_contract` (`script[35] == SYSCALL`). *)
Lemma redeem_script_syscall :
  forall pk, length pk = 33 ->
    nth 35 (signature_redeem_script pk) 0 = 0x68.
Proof.
  intros pk Hlen.
  unfold signature_redeem_script.
  assert (H35 : 35 >= length [0x0C;33]) by (simpl; lia).
  rewrite (@app_nth2 nat [0x0C;33] (pk ++ [0x68] ++ check_sig_hash) 0 35 H35).
  change (35 - length [0x0C;33]) with 33.
  assert (H33 : 33 >= length pk) by lia.
  rewrite (@app_nth2 nat pk ([0x68] ++ check_sig_hash) 0 33 H33).
  rewrite Hlen. simpl. reflexivity.
Qed.

(** ---- Script-hash purity over any verification script ---- *)

(** THEOREM: the script-hash length does not depend on the input length. *)
Theorem script_hash_length_invariant :
  forall input_short input_long : bytes,
    length (compute_script_hash input_short) = UINT160_LENGTH /\
    length (compute_script_hash input_long) = UINT160_LENGTH.
Proof.
  intros input_short input_long.
  split; apply script_hash_is_uint160.
Qed.

(** Concrete examples. *)
Example empty_script_hash_uint160 : length (compute_script_hash []) = UINT160_LENGTH.
Proof. apply script_hash_is_uint160. Qed.

Example large_script_hash_uint160 :
  length (compute_script_hash (repeat 0 1024)) = UINT160_LENGTH.
Proof. apply script_hash_is_uint160. Qed.

Example signature_redeem_33_bytes_40 : length (signature_redeem_script (repeat 0 33)) = 40.
Proof.
  apply redeem_script_length_valid. reflexivity.
Qed.

(** Concrete: a compressed ECDSA public key (33 bytes) maps to a 40-byte script. *)
Example compressed_key_redeem_40 :
  length (signature_redeem_script ([2] ++ repeat 0 32)) = 40.
Proof.
  apply redeem_script_length_valid. simpl. lia.
Qed.

End WitnessScriptHash.
