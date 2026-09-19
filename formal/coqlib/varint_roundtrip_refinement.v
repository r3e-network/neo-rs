(** Neo N3 compact integers, checked with Coq 8.18.
    Manual correspondence: neo-io/src/var_int.rs read_var_int_prefix,
    write_var_int, encoded_len. This is not an extracted Rust refinement.

    Repairs: use binary Z arithmetic (not enormous unary naturals), standard
    imports and boolean comparisons; thresholds are 253, 65536, 4294967296.
    Decode all eight u64 bytes, return the consumed width, allow suffixes and
    legacy noncanonical prefixes, and reject incomplete prefixes. Roundtrip
    and canonicality require 0 <= n < 2^64: unbounded integers cannot roundtrip
    through eight bytes. Input bytes are Z with an explicit byte-validity domain;
    read_le, like Rust's &[u8], assumes byte-valid input. No axioms are used. *)
From Coq Require Import List ZArith Lia.
Import ListNotations.
Open Scope Z_scope.

Definition VAR_INT_U16_MARKER : Z := 253.
Definition VAR_INT_U32_MARKER : Z := 254.
Definition VAR_INT_U64_MARKER : Z := 255.
Definition u64_bound : Z := 18446744073709551616.
Definition valid_u64 (n : Z) := 0 <= n < u64_bound.
Definition valid_byte (b : Z) := 0 <= b < 256.

Fixpoint capacity (k : nat) : Z :=
  match k with O => 1 | S j => 256 * capacity j end.
Fixpoint le_encode (k : nat) (n : Z) : list Z :=
  match k with O => [] | S j => n mod 256 :: le_encode j (n / 256) end.
Fixpoint le_value (bs : list Z) : Z :=
  match bs with [] => 0 | b :: tl => b + 256 * le_value tl end.
Fixpoint read_le (k : nat) (bs : list Z) : option (Z * list Z) :=
  match k, bs with
  | O, _ => Some (0, bs)
  | S j, b :: tl =>
      match read_le j tl with
      | Some (n, rest) => Some (b + 256 * n, rest)
      | None => None
      end
  | _, _ => None
  end.

Lemma le_encode_length : forall k n, length (le_encode k n) = k.
Proof. induction k; intros; simpl; auto. Qed.
Lemma le_encode_bytes : forall k n, Forall valid_byte (le_encode k n).
Proof.
  induction k; intros; simpl; constructor; auto.
  unfold valid_byte; apply Z.mod_pos_bound; lia.
Qed.
Lemma le_value_encode : forall k n,
  0 <= n < capacity k -> le_value (le_encode k n) = n.
Proof.
  induction k; intros n H.
  - change (0 <= n < 1) in H. change (0 = n). lia.
  - change (0 <= n < 256 * capacity k) in H.
    change (n mod 256 + 256 * le_value (le_encode k (n / 256)) = n).
    assert (Hd : 0 <= n / 256 < capacity k).
    { split; [apply Z.div_pos|apply Z.div_lt_upper_bound]; lia. }
    rewrite (IHk _ Hd).
    pose proof (Z.div_mod n 256 ltac:(lia)). lia.
Qed.
Lemma read_le_encode : forall k n rest,
  read_le k (le_encode k n ++ rest) = Some (le_value (le_encode k n), rest).
Proof.
  induction k; intros; simpl; auto. rewrite IHk. reflexivity.
Qed.
Lemma read_le_short : forall k bs,
  (length bs < k)%nat -> read_le k bs = None.
Proof.
  induction k; intros bs H; [lia|]. destruct bs; simpl; auto.
  rewrite IHk; [reflexivity|simpl in H; lia].
Qed.
Lemma read_le_consumption : forall k bs n rest,
  read_le k bs = Some (n, rest) -> length bs = (k + length rest)%nat.
Proof.
  induction k; intros bs n rest H; simpl in H.
  - inversion H; reflexivity.
  - destruct bs; [discriminate|].
    destruct (read_le k bs) as [[v tail]|] eqn:E; [|discriminate].
    inversion H; subst. simpl. f_equal. eapply IHk; eauto.
Qed.

Definition varint_encode (n : Z) : list Z :=
  if n <? 253 then [n]
  else if n <? 65536 then 253 :: le_encode 2 n
  else if n <? 4294967296 then 254 :: le_encode 4 n
  else 255 :: le_encode 8 n.
Definition varint_encoded_length (n : Z) : nat :=
  if n <? 253 then 1%nat
  else if n <? 65536 then 3%nat
  else if n <? 4294967296 then 5%nat else 9%nat.
Definition read_payload (k : nat) (bs : list Z) : option (Z * nat) :=
  match read_le k bs with
  | Some (n, _) => Some (n, S k)
  | None => None
  end.
Definition read_var_int_prefix (bs : list Z) : option (Z * nat) :=
  match bs with
  | [] => None
  | b :: tl =>
      if b =? 253 then read_payload 2 tl
      else if b =? 254 then read_payload 4 tl
      else if b =? 255 then read_payload 8 tl
      else Some (b, 1%nat)
  end.
Definition varint_decode (bs : list Z) : option Z :=
  option_map fst (read_var_int_prefix bs).

Theorem varint_length_conservation : forall n,
  length (varint_encode n) = varint_encoded_length n.
Proof.
  intros n; unfold varint_encode, varint_encoded_length.
  destruct (n <? 253), (n <? 65536), (n <? 4294967296);
    simpl; reflexivity.
Qed.

Theorem varint_prefix_roundtrip : forall n rest, valid_u64 n ->
  read_var_int_prefix (varint_encode n ++ rest) =
    Some (n, varint_encoded_length n).
Proof.
  intros n rest H; unfold valid_u64, u64_bound in H.
  unfold varint_encode, varint_encoded_length.
  destruct (n <? 253) eqn:A; [apply Z.ltb_lt in A|apply Z.ltb_ge in A].
  - simpl; unfold read_var_int_prefix.
    assert (E1 : (n =? 253) = false) by (apply Z.eqb_neq; lia).
    assert (E2 : (n =? 254) = false) by (apply Z.eqb_neq; lia).
    assert (E3 : (n =? 255) = false) by (apply Z.eqb_neq; lia).
    rewrite E1, E2, E3; reflexivity.
  - destruct (n <? 65536) eqn:B; [apply Z.ltb_lt in B|apply Z.ltb_ge in B].
    + change (read_payload 2 (le_encode 2 n ++ rest) = Some (n, 3%nat)).
      unfold read_payload. rewrite read_le_encode, le_value_encode; [reflexivity|].
      cbn [capacity]; lia.
    + destruct (n <? 4294967296) eqn:C; [apply Z.ltb_lt in C|apply Z.ltb_ge in C].
      * change (read_payload 4 (le_encode 4 n ++ rest) = Some (n, 5%nat)).
        unfold read_payload. rewrite read_le_encode, le_value_encode; [reflexivity|].
        cbn [capacity]; lia.
      * change (read_payload 8 (le_encode 8 n ++ rest) = Some (n, 9%nat)).
        unfold read_payload. rewrite read_le_encode, le_value_encode; [reflexivity|].
        cbn [capacity]; lia.
Qed.
Theorem varint_roundtrip : forall n, valid_u64 n ->
  varint_decode (varint_encode n) = Some n.
Proof.
  intros n H. unfold varint_decode.
  pose proof (varint_prefix_roundtrip n [] H) as E.
  rewrite app_nil_r in E. rewrite E. reflexivity.
Qed.
Lemma varint_roundtrip_small : forall n, 0 <= n <= 252 ->
  varint_decode (varint_encode n) = Some n.
Proof. intros; apply varint_roundtrip; unfold valid_u64, u64_bound; lia. Qed.
Lemma varint_roundtrip_2byte : forall n, 253 <= n -> n <= 65535 ->
  varint_decode (varint_encode n) = Some n.
Proof. intros; apply varint_roundtrip; unfold valid_u64, u64_bound; lia. Qed.
Lemma varint_roundtrip_4byte : forall n, 65536 <= n -> n <= 4294967295 ->
  varint_decode (varint_encode n) = Some n.
Proof. intros; apply varint_roundtrip; unfold valid_u64, u64_bound; lia. Qed.
Lemma varint_roundtrip_8byte : forall n, 4294967296 <= n -> n < u64_bound ->
  varint_decode (varint_encode n) = Some n.
Proof. intros; apply varint_roundtrip; unfold valid_u64; lia. Qed.

(** Minimal-width format, including the previously vacuous eight-byte case. *)
Definition canonical_format (bs : list Z) : Prop :=
  Forall valid_byte bs /\
  match bs with
  | [] => False
  | b :: tl =>
    if b =? 253 then length tl = 2%nat /\ 253 <= le_value tl < 65536
    else if b =? 254 then length tl = 4%nat /\ 65536 <= le_value tl < 4294967296
    else if b =? 255 then length tl = 8%nat /\ 4294967296 <= le_value tl < u64_bound
    else tl = [] /\ 0 <= b < 253
  end.
Lemma canonical_encoding_rule : forall n, valid_u64 n -> canonical_format (varint_encode n).
Proof.
  intros n H; unfold valid_u64, u64_bound in H.
  unfold varint_encode, canonical_format.
  destruct (n <? 253) eqn:A; [apply Z.ltb_lt in A|apply Z.ltb_ge in A].
  - assert (E1 : (n =? 253) = false) by (apply Z.eqb_neq; lia).
    assert (E2 : (n =? 254) = false) by (apply Z.eqb_neq; lia).
    assert (E3 : (n =? 255) = false) by (apply Z.eqb_neq; lia).
    rewrite E1, E2, E3.
    split; [repeat constructor; unfold valid_byte; lia|].
    simpl; split; [reflexivity|lia].
  - destruct (n <? 65536) eqn:B; [apply Z.ltb_lt in B|apply Z.ltb_ge in B].
    + split; [constructor; [unfold valid_byte; lia|apply le_encode_bytes]|].
      change (length (le_encode 2 n) = 2%nat /\ 253 <= le_value (le_encode 2 n) < 65536).
      rewrite le_encode_length.
      rewrite (le_value_encode 2 n); [split; lia|cbn [capacity]; lia].
    + destruct (n <? 4294967296) eqn:C; [apply Z.ltb_lt in C|apply Z.ltb_ge in C].
      * split; [constructor; [unfold valid_byte; lia|apply le_encode_bytes]|].
        change (length (le_encode 4 n) = 4%nat /\ 65536 <= le_value (le_encode 4 n) < 4294967296).
        rewrite le_encode_length.
        rewrite (le_value_encode 4 n); [split; lia|cbn [capacity]; lia].
      * split; [constructor; [unfold valid_byte; lia|apply le_encode_bytes]|].
        change (length (le_encode 8 n) = 8%nat /\ 4294967296 <= le_value (le_encode 8 n) < u64_bound).
        rewrite le_encode_length.
        rewrite (le_value_encode 8 n); unfold u64_bound; [split; lia|cbn [capacity]; lia].
Qed.

Example rust_varint_fc : varint_encode 252 = [252]. Proof. reflexivity. Qed.
Example rust_varint_fd : varint_encode 253 = [253; 253; 0]. Proof. reflexivity. Qed.
Example rust_varint_max_u16 : varint_encode 65535 = [253; 255; 255]. Proof. reflexivity. Qed.
Example rust_varint_u32_boundary : varint_encode 65536 = [254; 0; 0; 1; 0]. Proof. reflexivity. Qed.
Example rust_varint_max_u32 : varint_encode 4294967295 = [254; 255; 255; 255; 255]. Proof. reflexivity. Qed.
Example rust_varint_u64_boundary : varint_encode 4294967296 = [255; 0; 0; 0; 0; 1; 0; 0; 0]. Proof. reflexivity. Qed.
Example rust_varint_max_u64 : varint_encode 18446744073709551615 = [255; 255; 255; 255; 255; 255; 255; 255; 255]. Proof. reflexivity. Qed.
Theorem rust_encoded_length_matches :
  varint_encoded_length 0 = 1%nat /\ varint_encoded_length 252 = 1%nat /\
  varint_encoded_length 253 = 3%nat /\ varint_encoded_length 65535 = 3%nat /\
  varint_encoded_length 65536 = 5%nat /\ varint_encoded_length 4294967295 = 5%nat /\
  varint_encoded_length 4294967296 = 9%nat.
Proof. repeat split; reflexivity. Qed.
Example legacy_noncanonical_prefixes :
  read_var_int_prefix [253; 1; 0] = Some (1, 3%nat) /\
  read_var_int_prefix [254; 1; 0; 0; 0] = Some (1, 5%nat) /\
  read_var_int_prefix [255; 1; 0; 0; 0; 0; 0; 0; 0] = Some (1, 9%nat).
Proof. repeat split; reflexivity. Qed.
Example incomplete_prefixes :
  read_var_int_prefix [] = None /\ read_var_int_prefix [253] = None /\
  read_var_int_prefix [253; 1] = None /\ read_var_int_prefix [254; 0; 0; 0] = None /\
  read_var_int_prefix [255; 0; 0; 0; 0; 0; 0; 0] = None.
Proof. repeat split; reflexivity. Qed.
