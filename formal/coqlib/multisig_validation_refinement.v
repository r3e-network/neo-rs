From Coq Require Import List Arith Lia Bool.
Import ListNotations.

(** Multi-signature structural validation aligned with neo-core/src/witness.rs
    `verify_multi_signature` and neo-core/src/smart_contract/helper.rs
    (Neo N3).  SHA-256, RIPEMD-160, the CheckSig syscall hash and the bytewise
    key ordering are explicit abstract Section parameters; no claim of
    refinement against the concrete crypto or Rust `Vec<u8>::sort`.

    Honest corrections vs the legacy model:
      * the single-signature redeem script is the 40-byte N3 layout
        `PUSHDATA1(0x0C) 33 <33-byte key> SYSCALL(0x68) <4-byte CheckSig hash>`,
        NOT the 35-byte Neo2 layout `0x21 <key> 0xAC`;
      * `account_binding` is a real guard (`UInt160::from_script(script) !=
        account` -> reject), modeled as a predicate; the old claim that
        `account = compute_multisig_hash m n public_keys` with
        `compute_multisig_hash := []` (i.e. `account = []`) is FALSE and is
        dropped;
      * `is_sorted := True` placeholder is replaced by a real ascending
        predicate.
*)

Section MultiSigValidation.

Definition bytes := list nat.
Definition public_key := bytes.
Definition signature := bytes.
Definition message := bytes.

Variable sha256_abstract : bytes -> bytes.
Variable ripemd160_abstract : bytes -> bytes.
Hypothesis sha256_output_size : forall b, length (sha256_abstract b) = 32.
Hypothesis ripemd160_output_size : forall b, length (ripemd160_abstract b) = 20.

Definition compute_script_hash (script : bytes) : bytes :=
  ripemd160_abstract (sha256_abstract script).

Definition valid_uint160 (h : bytes) : Prop := length h = 20.

(** ---- 40-byte N3 single-signature redeem script ---- *)
Variable check_sig_hash : bytes.
Hypothesis check_sig_hash_size : length check_sig_hash = 4.

Definition signature_redeem_script (pk : public_key) : bytes :=
  [0x0C; 33] ++ pk ++ [0x68] ++ check_sig_hash.

(** THEOREM: a 33-byte compressed public key yields a 40-byte redeem script
    (corrects the legacy 35-byte claim). *)
Lemma signature_redeem_script_40 :
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

(** THEOREM: position 35 is the SYSCALL opcode (0x68). *)
Lemma redeem_script_syscall :
  forall pk, length pk = 33 ->
    nth 35 (signature_redeem_script pk) 0 = 0x68.
Proof.
  intros pk Hlen.
  unfold signature_redeem_script.
  assert (H35 : 35 >= length [0x0C;33]) by (simpl; lia).
  rewrite (@app_nth2 nat [0x0C;33] (pk ++ [0x68] ++ check_sig_hash) 0 35 H35).
  change (35 - length [0x0C;33]) with 33.
  change (pk ++ [0x68] ++ check_sig_hash) with (pk ++ ([0x68] ++ check_sig_hash)).
  assert (H33 : 33 >= length pk) by lia.
  rewrite (@app_nth2 nat pk ([0x68] ++ check_sig_hash) 0 33 H33).
  rewrite Hlen. simpl. reflexivity.
Qed.

(** ---- Script hash properties ---- *)

(** THEOREM: the script hash is always 20 bytes (a valid UInt160). *)
Lemma script_hash_is_uint160 :
  forall script, length (compute_script_hash script) = 20.
Proof.
  intros script. unfold compute_script_hash. rewrite ripemd160_output_size. reflexivity.
Qed.

(** THEOREM: the script hash is deterministic (pure function). *)
Lemma script_hash_deterministic :
  forall s1 s2, s1 = s2 -> compute_script_hash s1 = compute_script_hash s2.
Proof.
  intros s1 s2 Heq. subst. reflexivity.
Qed.

(** ---- Multi-sig threshold ---- *)

Definition max_keys : nat := 1024.

Definition valid_multisig_params (m n : nat) : Prop :=
  1 <= m /\ m <= n /\ n <= max_keys.

(** THEOREM: threshold correctness (real predicate, not a placeholder). *)
Lemma threshold_correctness :
  forall m n, valid_multisig_params m n <-> 1 <= m /\ m <= n /\ n <= max_keys.
Proof. intros. reflexivity. Qed.

(** ---- Real ascending sort predicate ---- *)

(** Strict bytewise total order over public keys (abstract; mirrors
    std::cmp::Ord on Vec<u8>). *)
Variable key_lt : public_key -> public_key -> bool.

Fixpoint is_sorted (l : list public_key) : Prop :=
  match l with
  | a :: l0 =>
      match l0 with
      | b :: rest => (key_lt a b = true \/ a = b) /\ is_sorted l0
      | _ => True
      end
  | _ => True
  end.

(** THEOREM: a singleton list is sorted. *)
Lemma single_sorted : forall k, is_sorted [k].
Proof. intros. reflexivity. Qed.

(** THEOREM: an empty list is sorted. *)
Lemma empty_sorted : is_sorted [].
Proof. reflexivity. Qed.

(** ---- Under-signature rejection (real guard, non-vacuous) ---- *)

Definition params_valid_bool (m n : nat) : bool :=
  Nat.leb 1 m && Nat.leb m n && Nat.leb n max_keys.

Definition verify_multi_signature (m : nat)
                                  (keys : list public_key) (sigs : list signature) : bool :=
  params_valid_bool m (length keys) && Nat.eqb (length sigs) m.

(** THEOREM: fewer than m signatures is rejected. *)
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

(** ---- Account binding (honest guard, not the false `account = []`) ---- *)

(** The real guard is `UInt160::from_script(&script) != *account` -> reject;
    i.e. acceptance requires `compute_script_hash script = account`. *)
Definition account_matches (account : bytes) (script : bytes) : Prop :=
  compute_script_hash script = account.

(** THEOREM: account binding is stable under equal scripts. *)
Lemma account_matches_deterministic :
  forall account s1 s2, s1 = s2 ->
    account_matches account s1 -> account_matches account s2.
Proof.
  intros account s1 s2 Heq Hm.
  subst. exact Hm.
Qed.

(** THEOREM: the script hash used for account binding is a valid UInt160. *)
Lemma account_matches_hash_is_uint160 :
  forall account script, account_matches account script -> valid_uint160 account.
Proof.
  intros account script Hm. unfold account_matches, valid_uint160 in *.
  subst. apply script_hash_is_uint160.
Qed.

(** ---- Multi-sig redeem script size (structural upper bound) ---- *)

(** Upper bound on a canonical multi-sig redeem script given m-of-n keys:
    each key contributes PUSHDATA1 + len + 33 bytes = 35; plus two small pushes
    and a 5-byte tail (SYSCALL + 4-byte CheckMultisig hash). *)
Definition multisig_script_size_bound (n : nat) : nat :=
  35 * n + 7.

(** Honest bound: a key set fits the 1024-byte verification-script cap exactly
    when 35*n+7 < 1024, i.e. n <= 29.  (The old blanket claim for all n <= 1024
    is false: 35*1024+7 far exceeds the cap, so it is not stated.) *)
Lemma multisig_size_bound_small :
  forall n, n <= 29 -> multisig_script_size_bound n < 1024.
Proof.
  intros n Hn.
  unfold multisig_script_size_bound.
  lia.
Qed.

(** Example: the 21-key committee layout (m=11, n=21) is well within the cap. *)
Example committee_21_keys_fits : multisig_script_size_bound 21 < 1024.
Proof.
  unfold multisig_script_size_bound. lia.
Qed.

End MultiSigValidation.
