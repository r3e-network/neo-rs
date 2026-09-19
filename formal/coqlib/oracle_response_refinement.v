(**
  Oracle Response Validation — Coq 8.18 honest rebuild.

  Rust source correspondence (manual, abstract; NOT a mechanical refinement):
    neo-core/src/smart_contract/native/oracle_contract/mod.rs::PendingRequest
      and response-code / size handling.

  This is an ABSTRACT model of the oracle request/response validation:
  request field length bounds, the response gas floor, response-code validity,
  and the success-vs-error result size rule.  Machine-checked:
    - request URL/callback/user-data lengths respect protocol bounds,
    - the callback length is positive and bounded,
    - gas is floored at MIN_RESPONSE_GAS,
    - an error response carries an empty result; a success response is bounded
      by MAX_RESPONSE_LENGTH.

  Boundaries:
    - Strings are modeled by lengths only; JSONPath content, callback name
      validity, off-chain fetch, and cryptographic signature verification are
      out of scope (no such claims are made).
    - No claim that the Rust/c# oracle contract is refined.
*)
From Coq Require Import Arith Lia List.
Import ListNotations.
Open Scope nat_scope.

(* Protocol bounds in arithmetic-product form so lia can reason about them
   (Coq 8.18 parses large nat literals as of_num_uint which breaks lia). *)
Definition MAX_URL_LENGTH : nat := 2*2*2*2*2*2*2*2.          (* 256 *)
Definition MAX_CALLBACK_LENGTH : nat := 2*2*2*2*2.           (* 32 *)
Definition MAX_USER_DATA_LENGTH : nat := 2*2*2*2*2*2*2*2*2.  (* 512 *)
Definition MAX_RESPONSE_LENGTH : nat := 256*256.              (* 65536 *)
Definition MIN_RESPONSE_GAS : nat := 10*10*10*10*10*10*10.    (* 1e7 *)

(* Oracle response code enum values. *)
Definition OracleRespCode_Success : nat := 0.
Definition OracleRespCode_Error : nat := 1.
Definition OracleRespCode_Failure : nat := 2.
Definition OracleRespCode_PermissionDenied : nat := 3.
Definition OracleRespCode_UnsupportedFeature : nat := 4.

Definition is_valid_resp_code (c : nat) : Prop :=
  c = 0 \/ c = 1 \/ c = 2 \/ c = 3 \/ c = 4.

(* An oracle request, with unbounded fields abstracted to their lengths. *)
Record OracleRequest := make_oracle_request {
  request_id       : nat;
  gas_for_response : nat;
  url_len          : nat;
  callback_len     : nat;
  user_data_len    : nat
}.

Definition validate_oracle_request (req : OracleRequest) : Prop :=
  req.(url_len) <= MAX_URL_LENGTH /\
  req.(callback_len) > 0 /\
  req.(callback_len) <= MAX_CALLBACK_LENGTH /\
  req.(user_data_len) <= MAX_USER_DATA_LENGTH /\
  req.(gas_for_response) >= MIN_RESPONSE_GAS.

(* A response is well-formed per its code.  Error responses carry empty data;
   success carries a bounded result. *)
Definition validate_oracle_response (code : nat) (result_len : nat) : Prop :=
  is_valid_resp_code code /\
  match code with
  | 0 => result_len <= MAX_RESPONSE_LENGTH
  | _ => result_len = 0
  end.

(** --- Response code / size theorems --- *)

Lemma resp_code_valid_bounded :
  forall code result_len,
    validate_oracle_response code result_len -> code <= 4.
Proof.
  intros code result_len H.
  destruct H as [Hcode _].
  unfold is_valid_resp_code in Hcode.
  repeat (destruct Hcode as [H | Hcode]; [subst; lia |]).
  subst. lia.
Qed.

Lemma success_bounded :
  forall result_len,
    validate_oracle_response 0 result_len -> result_len <= MAX_RESPONSE_LENGTH.
Proof.
  intros result_len H. destruct H as [_ Hm]. exact Hm.
Qed.

Lemma error_empty :
  forall code result_len,
    is_valid_resp_code code -> code <> 0 ->
    validate_oracle_response code result_len -> result_len = 0.
Proof.
  intros code result_len Hvalid Hnz H.
  destruct H as [_ Hm].
  destruct code as [|[|[|[|[|extra]]]]].
  - exfalso. apply Hnz. reflexivity.
  - exact Hm.
  - exact Hm.
  - exact Hm.
  - exact Hm.
  - exact Hm.
Qed.

Lemma resp_codes_in_enum :
  is_valid_resp_code 0 /\ is_valid_resp_code 1 /\ is_valid_resp_code 2 /\
  is_valid_resp_code 3 /\ is_valid_resp_code 4.
Proof.
  unfold is_valid_resp_code. auto 99.
Qed.

(** --- Request bound theorems --- *)

Lemma url_bound : forall req, validate_oracle_request req -> req.(url_len) <= MAX_URL_LENGTH.
Proof. intros req H. destruct H as [Hurl _]. exact Hurl. Qed.

Lemma callback_nonempty : forall req, validate_oracle_request req -> req.(callback_len) > 0.
Proof. intros req H. destruct H as [_ [Hcb _]]. exact Hcb. Qed.

Lemma callback_bound : forall req, validate_oracle_request req -> req.(callback_len) <= MAX_CALLBACK_LENGTH.
Proof. intros req H. destruct H as [_ [_ [Hcb _]]]. exact Hcb. Qed.

Lemma user_data_bound : forall req, validate_oracle_request req -> req.(user_data_len) <= MAX_USER_DATA_LENGTH.
Proof. intros req H. destruct H as [_ [_ [_ [Hud _]]]]. exact Hud. Qed.

Lemma gas_floor_enforced : forall req, validate_oracle_request req -> req.(gas_for_response) >= MIN_RESPONSE_GAS.
Proof. intros req H. destruct H as [_ [_ [_ [_ Hg]]]]. exact Hg. Qed.

(** Concrete examples. *)
Section Examples.

Definition example_request :=
  make_oracle_request 1001 MIN_RESPONSE_GAS 10 5 3.

Theorem example_request_valid : validate_oracle_request example_request.
Proof.
  unfold example_request, validate_oracle_request.
  cbn [url_len callback_len user_data_len gas_for_response].
  unfold MAX_URL_LENGTH, MAX_CALLBACK_LENGTH, MAX_USER_DATA_LENGTH, MIN_RESPONSE_GAS.
  repeat (split; try lia).
Qed.

Theorem example_success_bounded : validate_oracle_response 0 1000.
Proof.
  unfold validate_oracle_response, is_valid_resp_code.
  unfold MAX_RESPONSE_LENGTH.
  split; [left; reflexivity | lia].
Qed.

End Examples.