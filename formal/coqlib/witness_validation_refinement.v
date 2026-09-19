From Coq Require Import List Arith Lia Bool.
Import ListNotations.

(** Witness validation aligned with neo-core/src/witness.rs (`verify_signature`,
    `verify_multi_signature`, `Witness::script_hash`) and
    neo-core/src/smart_contract/helper.rs (40-byte N3 redeem script).  SHA-256,
    RIPEMD-160, the CheckSig syscall hash and the Secp256r1 ECDSA oracle are
    explicit abstract Section parameters; no claim of refinement against the
    concrete crypto.

    Honest corrections vs the legacy model:
      * the single-signature redeem script is the 40-byte N3 layout, not the
        35-byte Neo2 layout (all `= 35` claims are corrected to `= 40`);
      * the size bounds are model as a `valid_witness` predicate (and its
        consequences), not as unprovable axioms about arbitrary witnesses;
      * fewer-than-m signatures is a REAL guard (`signatures.len() != m` ->
        reject), so the legacy "无欠签名" property is non-vacuous.  The old axiom
        `multisig_provided_sufficient` (provided >= required) made it vacuous
        and is dropped.
*)

Section WitnessValidation.

Definition bytes := list nat.
Definition public_key := bytes.
Definition signature := bytes.
Definition message := bytes.

Definition UINT160_LENGTH : nat := 20.
Definition UINT256_LENGTH : nat := 32.
Definition MAX_INVOCATION_SCRIPT : nat := 1024.
Definition MAX_VERIFICATION_SCRIPT : nat := 1024.

Variable sha256_abstract : bytes -> bytes.
Variable ripemd160_abstract : bytes -> bytes.
Hypothesis sha256_output_size : forall b, length (sha256_abstract b) = UINT256_LENGTH.
Hypothesis ripemd160_output_size : forall b, length (ripemd160_abstract b) = UINT160_LENGTH.

Definition compute_script_hash (script : bytes) : bytes :=
  ripemd160_abstract (sha256_abstract script).

(** THEOREM: the script hash is a UInt160 (20 bytes). *)
Theorem script_hash_size_correct :
  forall script, length (compute_script_hash script) = UINT160_LENGTH.
Proof.
  intros script. unfold compute_script_hash. rewrite ripemd160_output_size. reflexivity.
Qed.

(** THEOREM: the script hash is deterministic. *)
Theorem script_hash_deterministic :
  forall s1 s2, s1 = s2 -> compute_script_hash s1 = compute_script_hash s2.
Proof.
  intros s1 s2 Heq. subst. reflexivity.
Qed.

(** ---- Witness structural bounds (predicate, not axioms) ---- *)

Record Witness : Type := make_witness {
  invocation_script   : bytes;
  verification_script : bytes;
}.

Definition valid_witness (w : Witness) : Prop :=
  length (invocation_script w) <= MAX_INVOCATION_SCRIPT /\
  length (verification_script w) <= MAX_VERIFICATION_SCRIPT.

(** THEOREM: a valid witness keeps its invocation script within the cap. *)
Lemma valid_witness_invocation_bound :
  forall w, valid_witness w -> length (invocation_script w) <= MAX_INVOCATION_SCRIPT.
Proof.
  intros w Hv. destruct Hv as [H1 _]. exact H1.
Qed.

(** THEOREM: a valid witness keeps its verification script within the cap. *)
Lemma valid_witness_verification_bound :
  forall w, valid_witness w -> length (verification_script w) <= MAX_VERIFICATION_SCRIPT.
Proof.
  intros w Hv. destruct Hv as [_ H2]. exact H2.
Qed.

(** ---- 40-byte N3 single-signature redeem script ---- *)

Variable check_sig_hash : bytes.
Hypothesis check_sig_hash_size : length check_sig_hash = 4.

Definition signature_redeem_script (pk : public_key) : bytes :=
  [0x0C; 33] ++ pk ++ [0x68] ++ check_sig_hash.

(** Boolean format check mirroring helper.rs `is_signature_contract`: the script
    is 40 bytes, opens PUSHDATA1(0x0C) 33, and has SYSCALL(0x68) at byte 35. *)
Definition sig_contract_ok (script : bytes) : bool :=
  Nat.eqb (length script) 40 &&
  Nat.eqb (nth 0 script 0) 0x0C &&
  Nat.eqb (nth 1 script 0) 33 &&
  Nat.eqb (nth 35 script 0) 0x68.

(** Auxiliary facts for the redeem-script format. *)
Lemma redeem_length :
  forall pk, length pk = 33 -> length (signature_redeem_script pk) = 40.
Proof.
  intros pk Hlen.
  unfold signature_redeem_script.
  rewrite !app_length. simpl. rewrite Hlen, check_sig_hash_size. lia.
Qed.

Lemma redeem_op0 : forall pk, nth 0 (signature_redeem_script pk) 0 = 0x0C.
Proof. intros pk. unfold signature_redeem_script. simpl. reflexivity. Qed.

Lemma redeem_op1 : forall pk, nth 1 (signature_redeem_script pk) 0 = 33.
Proof. intros pk. unfold signature_redeem_script. simpl. reflexivity. Qed.

Lemma redeem_op35 :
  forall pk, length pk = 33 -> nth 35 (signature_redeem_script pk) 0 = 0x68.
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

(** THEOREM: a 33-byte key yields a format-valid 40-byte redeem script. *)
Lemma sig_contract_ok_redeem :
  forall pk, length pk = 33 -> sig_contract_ok (signature_redeem_script pk) = true.
Proof.
  intros pk Hlen.
  unfold sig_contract_ok.
  rewrite (redeem_length pk Hlen).
  rewrite (redeem_op0 pk).
  rewrite (redeem_op1 pk).
  rewrite (redeem_op35 pk Hlen).
  simpl. reflexivity.
Qed.

(** THEOREM (corrected): a 33-byte compressed public key gives a 40-byte script. *)
Theorem redeem_script_40 :
  forall pk, length pk = 33 -> length (signature_redeem_script pk) = 40.
Proof. intros. apply redeem_length. exact H. Qed.

(** THEOREM: scripts of length other than 40 fail the single-signature format
    check (mirrors `is_signature_contract` returning false). *)
Lemma sig_contract_ok_rejects :
  forall script, length script <> 40 -> sig_contract_ok script = false.
Proof.
  intros script Hne.
  unfold sig_contract_ok.
  assert (Hb : Nat.eqb (length script) 40 = false).
  { apply (proj2 (Nat.eqb_neq (length script) 40)). lia. }
  rewrite Hb. simpl. reflexivity.
Qed.

(** ---- Single-signature verification (structural guards) ---- *)

Variable ecdsa_verify : message -> signature -> public_key -> bool.

(** Single-sig verification mirrors witness.rs `verify_signature`: it requires a
    40-byte signature contract, a 66-byte invocation script `[PUSHDATA1 0x40
    <64-byte sig>]`, and ECDSA verification of the extracted key/signature. *)
Definition verify_single_signature (msg : message) (w : Witness) : bool :=
  sig_contract_ok (verification_script w) &&
  Nat.eqb (length (invocation_script w)) 66 &&
  Nat.eqb (nth 0 (invocation_script w) 0) 0x0C &&
  Nat.eqb (nth 1 (invocation_script w) 0) 0x40 &&
  ecdsa_verify msg (skipn 2 (invocation_script w)) (skipn 2 (verification_script w)).

(** THEOREM: a witness whose verification script is not 40 bytes is rejected. *)
Lemma single_sig_rejects_bad_verification_len :
  forall msg w, length (verification_script w) <> 40 -> verify_single_signature msg w = false.
Proof.
  intros msg w Hne.
  unfold verify_single_signature.
  rewrite (sig_contract_ok_rejects (verification_script w) Hne).
  simpl. reflexivity.
Qed.

(** THEOREM: a witness whose invocation script is not 66 bytes is rejected. *)
Lemma single_sig_rejects_bad_invocation_len :
  forall msg w, length (invocation_script w) <> 66 -> verify_single_signature msg w = false.
Proof.
  intros msg w Hne.
  unfold verify_single_signature.
  assert (Hb : Nat.eqb (length (invocation_script w)) 66 = false).
  { apply (proj2 (Nat.eqb_neq (length (invocation_script w)) 66)). lia. }
  rewrite Hb.
  rewrite Bool.andb_false_r. simpl. reflexivity.
Qed.

(** ---- Multi-signature threshold enforcement (real guards) ---- *)

Definition max_keys : nat := 1024.

Definition valid_multisig_params (m n : nat) : Prop :=
  1 <= m /\ m <= n /\ n <= max_keys.

Definition params_valid_bool (m n : nat) : bool :=
  Nat.leb 1 m && Nat.leb m n && Nat.leb n max_keys.

(** Full multi-sig verification mirroring witness.rs `verify_multi_signature`
    guards: exactly m signatures are required, and 1 <= m <= n <= 1024. *)
Definition verify_multi_signature (m : nat)
                                  (keys : list public_key) (sigs : list signature) : bool :=
  params_valid_bool m (length keys) && Nat.eqb (length sigs) m.

(** THEOREM: fewer than m signatures is always rejected (non-vacuous; the real
    guard is `signatures.len() != m` -> reject). *)
Lemma under_sig_rejection :
  forall m keys sigs, length sigs < m -> verify_multi_signature m keys sigs = false.
Proof.
  intros m keys sigs Hless.
  unfold verify_multi_signature.
  assert (Hne : length sigs <> m) by lia.
  rewrite (proj2 (Nat.eqb_neq (length sigs) m) Hne).
  rewrite Bool.andb_false_r. simpl. reflexivity.
Qed.

(** THEOREM: a threshold larger than the key set is rejected. *)
Lemma over_threshold_rejection :
  forall m keys sigs, m > length keys -> verify_multi_signature m keys sigs = false.
Proof.
  intros m keys sigs Hgt.
  unfold verify_multi_signature.
  unfold params_valid_bool.
  destruct (Nat.leb m (length keys)) eqn:E; simpl.
  - apply Nat.leb_le in E. lia.
  - rewrite Bool.andb_false_r. simpl. reflexivity.
Qed.

(** THEOREM: zero threshold is rejected (`required_signatures == 0` -> false). *)
Lemma zero_threshold_rejection :
  forall keys sigs, verify_multi_signature 0 keys sigs = false.
Proof.
  intros. unfold verify_multi_signature, params_valid_bool. simpl. reflexivity.
Qed.

(** THEOREM: threshold correctness (real predicate). *)
Lemma threshold_correctness :
  forall m n, valid_multisig_params m n <-> 1 <= m /\ m <= n /\ n <= max_keys.
Proof. intros. reflexivity. Qed.

(** Concrete example: a 33-byte compressed key yields a format-valid 40-byte
    single-signature redeem script. *)
Example compressed_key_format_ok :
  sig_contract_ok (signature_redeem_script (repeat 0 33)) = true.
Proof.
  apply sig_contract_ok_redeem. reflexivity.
Qed.

End WitnessValidation.
