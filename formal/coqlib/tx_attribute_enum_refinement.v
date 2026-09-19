Require Import Arith.
Require Import Coq.Strings.String.
Require Import Coq.Lists.List.
Require Import Lia.
Import ListNotations.

(** Transaction Attribute Type — abstract enum + byte bijection.

    This is an abstract model of Neo N3 transaction attribute type codes.
    It proves the byte/enum bijection and round-trip on the five known
    variants.  It is NOT a refinement proof of the full Rust attribute
    handling; it models the enumeration and its wire codes only.
*)

Inductive TxAttrType : Type :=
| TX_ATTR_HIGH_PRIORITY    (* 0x01 *)
| TX_ATTR_ORACLE_RESPONSE  (* 0x11 *)
| TX_ATTR_NOT_VALID_BEFORE (* 0x20 *)
| TX_ATTR_CONFLICTS        (* 0x21 *)
| TX_ATTR_NOTARY_ASSISTED  (* 0x22 *).

Definition to_byte (e : TxAttrType) : nat :=
  match e with
  | TX_ATTR_HIGH_PRIORITY    => 0x01
  | TX_ATTR_ORACLE_RESPONSE  => 0x11
  | TX_ATTR_NOT_VALID_BEFORE => 0x20
  | TX_ATTR_CONFLICTS        => 0x21
  | TX_ATTR_NOTARY_ASSISTED  => 0x22
  end.

Definition from_byte (b : nat) : option TxAttrType :=
  if Nat.eqb b 0x01 then Some TX_ATTR_HIGH_PRIORITY
  else if Nat.eqb b 0x11 then Some TX_ATTR_ORACLE_RESPONSE
  else if Nat.eqb b 0x20 then Some TX_ATTR_NOT_VALID_BEFORE
  else if Nat.eqb b 0x21 then Some TX_ATTR_CONFLICTS
  else if Nat.eqb b 0x22 then Some TX_ATTR_NOTARY_ASSISTED
  else None.

Definition as_str (e : TxAttrType) : string :=
  match e with
  | TX_ATTR_HIGH_PRIORITY    => "HighPriority"
  | TX_ATTR_ORACLE_RESPONSE  => "OracleResponse"
  | TX_ATTR_NOT_VALID_BEFORE => "NotValidBefore"
  | TX_ATTR_CONFLICTS        => "Conflicts"
  | TX_ATTR_NOTARY_ASSISTED  => "NotaryAssisted"
  end.

Definition allows_multiple (e : TxAttrType) : Prop :=
  match e with
  | TX_ATTR_CONFLICTS => True
  | _ => False
  end.

(** No two variants share a byte code (injectivity of to_byte). *)
Lemma to_byte_injective :
  forall e1 e2 : TxAttrType,
    to_byte e1 = to_byte e2 -> e1 = e2.
Proof.
  intros e1 e2 H.
  destruct e1; destruct e2; simpl in H; try reflexivity; try discriminate.
Qed.

(** Round-trip: encode then decode restores the variant. *)
Lemma from_byte_to_byte_inverse :
  forall e : TxAttrType,
    from_byte (to_byte e) = Some e.
Proof.
  intros e.
  destruct e; simpl; reflexivity.
Qed.

(** Any byte that is not a known code decodes to None. *)
Lemma unknown_bytes_return_none :
  forall b : nat,
    ~ (exists e : TxAttrType, to_byte e = b) ->
    from_byte b = None.
Proof.
  intros b Hnknown.
  destruct b as [|b].
  - reflexivity.
  - unfold from_byte.
    destruct (Nat.eqb (S b) 0x01) eqn:E1.
    + apply Nat.eqb_eq in E1.
      exfalso. apply Hnknown. exists TX_ATTR_HIGH_PRIORITY. simpl. rewrite E1. reflexivity.
    + destruct (Nat.eqb (S b) 0x11) eqn:E2.
      * apply Nat.eqb_eq in E2.
        exfalso. apply Hnknown. exists TX_ATTR_ORACLE_RESPONSE. simpl. rewrite E2. reflexivity.
      * destruct (Nat.eqb (S b) 0x20) eqn:E3.
        -- apply Nat.eqb_eq in E3.
           exfalso. apply Hnknown. exists TX_ATTR_NOT_VALID_BEFORE. simpl. rewrite E3. reflexivity.
        -- destruct (Nat.eqb (S b) 0x21) eqn:E4.
           ++ apply Nat.eqb_eq in E4.
              exfalso. apply Hnknown. exists TX_ATTR_CONFLICTS. simpl. rewrite E4. reflexivity.
           ++ destruct (Nat.eqb (S b) 0x22) eqn:E5.
              ** apply Nat.eqb_eq in E5.
                 exfalso. apply Hnknown. exists TX_ATTR_NOTARY_ASSISTED. simpl. rewrite E5. reflexivity.
              ** reflexivity.
Qed.

(** Human-readable names are non-empty. *)
Lemma as_str_nonempty :
  forall e : TxAttrType,
    String.length (as_str e) > 0.
Proof.
  intros e. destruct e; simpl; lia.
Qed.

Lemma allows_multiple_conflicts :
  forall e : TxAttrType,
    allows_multiple e <-> e = TX_ATTR_CONFLICTS.
Proof.
  intros e. split.
  - intro Hm. destruct e; simpl in Hm; try contradiction. reflexivity.
  - intro Heq. subst. simpl. exact I.
Qed.

Definition refinement_correct : Prop :=
  (forall e : TxAttrType, from_byte (to_byte e) = Some e) /\
  (forall e1 e2 : TxAttrType, to_byte e1 = to_byte e2 -> e1 = e2) /\
  (forall b : nat, ~ (exists e : TxAttrType, to_byte e = b) -> from_byte b = None) /\
  (forall e : TxAttrType, String.length (as_str e) > 0).

Theorem refinement_theorem : refinement_correct.
Proof.
  unfold refinement_correct.
  split; [apply from_byte_to_byte_inverse |].
  split; [apply to_byte_injective |].
  split; [apply unknown_bytes_return_none |].
  apply as_str_nonempty.
Qed.

(** Concrete round-trip examples. *)
Example roundtrip_high_priority :
  from_byte (to_byte TX_ATTR_HIGH_PRIORITY) = Some TX_ATTR_HIGH_PRIORITY.
Proof. reflexivity. Qed.

Example roundtrip_conflicts :
  from_byte (to_byte TX_ATTR_CONFLICTS) = Some TX_ATTR_CONFLICTS.
Proof. reflexivity. Qed.

Example conflicts_allows_multiple : allows_multiple TX_ATTR_CONFLICTS.
Proof. simpl. exact I. Qed.
