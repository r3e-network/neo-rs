(** Neo N3 serialization composition, Coq 8.18.

    Manual source correspondence, NOT a mechanically proved Rust refinement:
    neo-io/src/var_int.rs, binary_writer.rs, serializable/{mod,helper}.rs;
    neo-consensus/src/messages/mod.rs and message_type.rs;
    neo-core/src/network/p2p/payloads/{block,transaction}/serialization.rs
    and transaction/mod.rs (HEADER_SIZE).

    This file is self-contained: the compact-integer codec it needs is inlined
    below (it used to be imported from varint_roundtrip_refinement, which
    breaks under the strict isolated-coqc gate because of compile ordering).
    The inlined codec is the same binary-Z model that passes on its own.

    Repairs to the old model:
    - Consensus codes are 00/20/21/30/40/41 (not 03/10/20/02/40).
    - A block header is its actual byte sequence, not one element containing
      its length.
    - Transaction fixed fields occupy 25 = 1+4+8+8+4 bytes.
    - ConcreteTypes.BlockSizeTxCount means item COUNT; its size contribution
      is encoded_len(count).
    - Remove global byte-validity and roundtrip axioms. Generic obligations
      are explicit conditional contracts, not facts about every Serializable.

    Scope: structural size equalities use unbounded mathematical size sums.
    Connection to Rust requires representable u64 counts, no usize overflow,
    successful component serialization and correct component size contracts.
    Semantic deserializer validation (fees, signers, hashes, witnesses, etc.)
    and full block/transaction deserialize roundtrip are not claimed. *)
From Coq Require Import List ZArith Lia.
Import ListNotations.
Open Scope Z_scope.

(** --- Inlined compact-integer codec (varint_roundtrip_refinement core). --- *)

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

(** --- Serialization composition. --- *)

Definition encoded_len := varint_encoded_length.

Definition var_array_count_size (count : nat) := encoded_len (Z.of_nat count).
Definition serialize_array (items : list (list Z)) : list Z :=
  varint_encode (Z.of_nat (length items)) ++ concat items.
Definition array_size (sizes : list nat) : nat :=
  (var_array_count_size (length sizes) + fold_right Nat.add 0%nat sizes)%nat.
Lemma concat_size : forall items : list (list Z),
  length (concat items) = fold_right Nat.add 0%nat (map (@length Z) items).
Proof. induction items; simpl; auto. rewrite app_length, IHitems; reflexivity. Qed.
Lemma array_size_conservation : forall items,
  length (serialize_array items) = array_size (map (@length Z) items).
Proof.
  intros. unfold serialize_array, array_size, var_array_count_size.
  rewrite app_length, varint_length_conservation, concat_size, map_length.
  reflexivity.
Qed.
Definition serialize_var_bytes (bs : list Z) :=
  varint_encode (Z.of_nat (length bs)) ++ bs.
Definition var_bytes_size (n : nat) := (var_array_count_size n + n)%nat.
Lemma var_bytes_size_conservation : forall bs,
  length (serialize_var_bytes bs) = var_bytes_size (length bs).
Proof.
  intros; unfold serialize_var_bytes, var_bytes_size, var_array_count_size.
  rewrite app_length, varint_length_conservation; reflexivity.
Qed.
(** Prefix framing roundtrip, independent of any structured payload validator. *)
Definition parse_var_bytes (wire : list Z) : option (list Z * list Z) :=
  match read_var_int_prefix wire with
  | None => None
  | Some (n, width) =>
      let rest := skipn width wire in
      if andb (0 <=? n) (n <=? Z.of_nat (length rest)) then
        Some (firstn (Z.to_nat n) rest, skipn (Z.to_nat n) rest)
      else None
  end.
Theorem var_bytes_roundtrip : forall bs suffix,
  valid_u64 (Z.of_nat (length bs)) ->
  parse_var_bytes (serialize_var_bytes bs ++ suffix) = Some (bs, suffix).
Proof.
  intros bs suffix H.
  unfold parse_var_bytes, serialize_var_bytes.
  rewrite <- app_assoc, (varint_prefix_roundtrip _ _ H).
  rewrite <- varint_length_conservation.
  rewrite skipn_app, skipn_all, Nat.sub_diag; simpl.
  rewrite app_length.
  assert (E1 : (0 <=? Z.of_nat (length bs)) = true) by (apply Z.leb_le; lia).
  assert (E2 : (Z.of_nat (length bs) <=? Z.of_nat (length bs + length suffix)) = true)
    by (apply Z.leb_le; lia).
  rewrite E1, E2; simpl. rewrite Nat2Z.id.
  rewrite firstn_app, firstn_all, Nat.sub_diag; simpl; rewrite app_nil_r.
  rewrite skipn_app, skipn_all, Nat.sub_diag; reflexivity.
Qed.

Inductive cons_msg_type := PReq | PRsp | ChangeView | Commit | RecoveryRequest | RecoveryMessage.
Definition from_byte (t : cons_msg_type) : Z :=
  match t with PReq => 32 | PRsp => 33 | ChangeView => 0 | Commit => 48
  | RecoveryRequest => 64 | RecoveryMessage => 65 end.
Definition encode_consensus_header (t : cons_msg_type) (block validator view : Z) : list Z :=
  [from_byte t] ++ le_encode 4 block ++ [validator; view].
Definition consensus_payload_total_length (body_length : nat) := (7 + body_length)%nat.
Theorem consensus_wire_length : forall t block validator view body_len,
  length (encode_consensus_header t block validator view) = 7%nat /\
  (7 + body_len)%nat = consensus_payload_total_length body_len.
Proof.
  intros; split; [unfold encode_consensus_header; repeat rewrite app_length;
    rewrite le_encode_length; reflexivity|reflexivity].
Qed.
Theorem consensus_body_length : forall t block validator view body,
  length (encode_consensus_header t block validator view ++ body) =
    consensus_payload_total_length (length body).
Proof.
  intros; rewrite app_length, (proj1 (consensus_wire_length t block validator view 0)).
  reflexivity.
Qed.
Lemma consensus_valid_bytes : forall t block validator view,
  valid_byte validator -> valid_byte view ->
  Forall valid_byte (encode_consensus_header t block validator view).
Proof.
  intros. unfold encode_consensus_header.
  apply Forall_app; split.
  - constructor; [destruct t; unfold from_byte, valid_byte; lia|constructor].
  - apply Forall_app; split.
    + apply le_encode_bytes.
    + constructor; [assumption|].
      constructor; [assumption|constructor].
Qed.
Example consensus_example_prsp : encode_consensus_header PRsp 42 9 1 = [33; 42; 0; 0; 0; 9; 1].
Proof. reflexivity. Qed.

Definition block_serialized_size (header_size : nat) (transaction_sizes : list nat) : nat :=
  (header_size + array_size transaction_sizes)%nat.
Definition block_actual_serialize_size (header : list Z) (transactions : list (list Z)) : list Z :=
  header ++ serialize_array transactions.
Theorem block_serialized_size_conservation : forall header transactions,
  length (block_actual_serialize_size header transactions) =
  block_serialized_size (length header) (map (@length Z) transactions).
Proof.
  intros; unfold block_actual_serialize_size, block_serialized_size.
  rewrite app_length, array_size_conservation; reflexivity.
Qed.
Definition block_serialize_checked header transactions :=
  if (length transactions <=? 65535)%nat
  then Some (block_actual_serialize_size header transactions) else None.
Theorem block_success_size : forall header transactions wire,
  block_serialize_checked header transactions = Some wire ->
  length wire = block_serialized_size (length header) (map (@length Z) transactions).
Proof.
  intros header transactions wire H; unfold block_serialize_checked in H.
  destruct (length transactions <=? 65535)%nat; inversion H; subst.
  apply block_serialized_size_conservation.
Qed.
Example block_example_three_txns :
  length (block_actual_serialize_size (repeat 0 80)
    [repeat 1 100; repeat 2 150; repeat 3 200]) = 531%nat.
Proof. vm_compute; reflexivity. Qed.

Definition HEADER_SIZE : nat := 25.
Record transaction_header := {
  tx_version : Z; tx_nonce : Z; tx_system_fee_bits : Z;
  tx_network_fee_bits : Z; tx_valid_until_block : Z
}.
Definition encode_transaction_header (h : transaction_header) : list Z :=
  [tx_version h] ++ le_encode 4 (tx_nonce h) ++
  le_encode 8 (tx_system_fee_bits h) ++ le_encode 8 (tx_network_fee_bits h) ++
  le_encode 4 (tx_valid_until_block h).
Lemma transaction_header_size : forall h,
  length (encode_transaction_header h) = HEADER_SIZE.
Proof.
  intros; unfold encode_transaction_header, HEADER_SIZE.
  repeat rewrite app_length; repeat rewrite le_encode_length; reflexivity.
Qed.
Definition transaction_declared_size (signer_sizes attribute_sizes : list nat)
    (script_len : nat) (witness_sizes : list nat) : nat :=
  (HEADER_SIZE + array_size signer_sizes + array_size attribute_sizes +
   var_bytes_size script_len + array_size witness_sizes)%nat.
Definition transaction_actual_serialize_size h signers attributes script witnesses : list Z :=
  encode_transaction_header h ++ serialize_array signers ++ serialize_array attributes ++
  serialize_var_bytes script ++ serialize_array witnesses.
Theorem transaction_size_conservation : forall h signers attributes script witnesses,
  length (transaction_actual_serialize_size h signers attributes script witnesses) =
  transaction_declared_size (map (@length Z) signers) (map (@length Z) attributes)
    (length script) (map (@length Z) witnesses).
Proof.
  intros; unfold transaction_actual_serialize_size, transaction_declared_size.
  repeat rewrite app_length. rewrite transaction_header_size.
  repeat rewrite array_size_conservation. rewrite var_bytes_size_conservation. lia.
Qed.
Definition transaction_serialize_checked h signers attributes script witnesses :=
  if (length script <=? 65535)%nat
  then Some (transaction_actual_serialize_size h signers attributes script witnesses) else None.
Theorem transaction_success_size : forall h signers attributes script witnesses wire,
  transaction_serialize_checked h signers attributes script witnesses = Some wire ->
  length wire = transaction_declared_size (map (@length Z) signers)
    (map (@length Z) attributes) (length script) (map (@length Z) witnesses).
Proof.
  intros h signers attributes script witnesses wire H.
  unfold transaction_serialize_checked in H.
  destruct (length script <=? 65535)%nat; inversion H; subst.
  apply transaction_size_conservation.
Qed.
(** Uniform-size scenario kept as a specialization, not the model. *)
Lemma uniform_array_size : forall count size,
  array_size (repeat size count) = (var_array_count_size count + count * size)%nat.
Proof.
  intros. unfold array_size. rewrite repeat_length. f_equal.
  induction count; simpl; auto.
Qed.
Example transaction_typical_case : forall h,
  length (transaction_actual_serialize_size h
    [repeat 1 40; repeat 2 40] [repeat 3 50] (repeat 4 100)
    [repeat 5 80; repeat 6 80]) = 419%nat.
Proof.
  intros. rewrite transaction_size_conservation. vm_compute; reflexivity.
Qed.

(** Trait laws are not guaranteed by Rust's Serializable trait definition.
    They are explicit premises; no unsound universal instances are asserted. *)
Section GenericSerializable.
  Context {T : Type}.
  Variable size : T -> nat.
  Variable serialize : T -> list Z.
  Variable deserialize : list Z -> option T.
  Definition serialization_contract :=
    (forall t, Forall valid_byte (serialize t)) /\
    (forall t, deserialize (serialize t) = Some t) /\
    (forall t, length (serialize t) = size t).
  Theorem serialize_valid_bytes : serialization_contract ->
    forall t b, In b (serialize t) -> valid_byte b.
  Proof.
    intros [B _] t b H. apply (proj1 (Forall_forall _ _) (B t)); assumption.
  Qed.
  Theorem deserialize_preserves_value : serialization_contract ->
    forall t, deserialize (serialize t) = Some t.
  Proof. intros [_ [R _]]; exact R. Qed.
  Theorem generic_length_conservation_property :
    (forall t, length (serialize t) = size t) -> forall t1 t2,
    length (serialize t1) = size t1 /\ length (serialize t2) = size t2.
  Proof. intros H t1 t2; split; apply H. Qed.
  (** Lift independent item size contracts through the actual array writer. *)
  Theorem generic_array_size :
    (forall t, length (serialize t) = size t) -> forall items,
    length (serialize_array (map serialize items)) = array_size (map size items).
  Proof.
    intros H items. rewrite array_size_conservation, map_map.
    assert (E : map (fun t => length (serialize t)) items = map size items).
    { apply map_ext. exact H. }
    rewrite E; reflexivity.
  Qed.
End GenericSerializable.

Module ConcreteTypes.
Section Block.
  Variable BlockHeader : list Z.
  Variable BlockTransactions : list (list Z).
  Definition BlockSizeHeader := length BlockHeader.
  Definition BlockSizeTxCount := length BlockTransactions.
  Definition BlockSizeTxTotal := fold_right Nat.add 0%nat (map (@length Z) BlockTransactions).
  Definition BlockSize :=
    (BlockSizeHeader + var_array_count_size BlockSizeTxCount + BlockSizeTxTotal)%nat.
  Definition BlockSerialize :=
    BlockHeader ++ varint_encode (Z.of_nat BlockSizeTxCount) ++ concat BlockTransactions.
  Theorem Block_size_matches_serialize : length BlockSerialize = BlockSize.
  Proof.
    unfold BlockSerialize, BlockSize, BlockSizeHeader, BlockSizeTxTotal,
           var_array_count_size, encoded_len.
    repeat rewrite app_length.
    rewrite varint_length_conservation, concat_size.
    lia.
  Qed.
End Block.
End ConcreteTypes.