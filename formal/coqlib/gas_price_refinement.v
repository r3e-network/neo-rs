(**
  Rust→Coq refinement (Phase-1): syscall gas pricing of neo-rs.

  Rust source mirrored:
    neo-vm/src/syscalls/cuckoo_hash.rs -> compute_gas_cost_for_method (match arms)
    neo-vm/src/syscalls/static_registry.rs -> get_gas_cost

  This covers the real syscall gas tiers (values are the C#/Rust `1 << N` costs)
  and proves the economic invariant that every syscall gas price is non-negative.
  A differential Rust test ties these to the live `get_gas_cost`.
*)
From Coq Require Import ZArith Lia.
Open Scope Z_scope.

Inductive SyscallCat :=
  | StorageContext          (* GetContext, GetReadOnlyContext, AsReadOnly      -> 1<<4  = 16    *)
  | StorageAccessors        (* Get, Put, Delete, Find                          -> 1<<15 = 32768 *)
  | RuntimePlatform         (* Platform .. CurrentSigners                      -> 1<<3  = 8     *)
  | RuntimeBurnGas          (* BurnGas, GetRandom                              -> 1<<4  = 16    *)
  | RuntimeCheckWitness     (* CheckWitness                                    -> 1<<10 = 1024  *)
  | RuntimeLogLoad          (* LoadScript, Log, Notify                         -> 1<<15 = 32768 *)
  | ContractCall            (* Call, Create, Update, CallNative                -> 1<<15 = 32768 *)
  | ContractGetCallFlags    (* GetCallFlags                                    -> 1<<4  = 16    *)
  | ContractCreateStd       (* CreateStandardAccount                           -> 1<<8  = 256   *)
  | ContractCreateMultisig  (* CreateMultisigAccount                           -> 1<<9  = 512   *)
  | ContractNativePersist   (* NativeOnPersist, NativePostPersist              -> 1<<10 = 1024  *)
  | CryptoCheck             (* CheckSig, CheckMultisig                         -> 1<<15 = 32768 *)
  | IteratorOps             (* Next, Value                                     -> 1<<15 = 32768 *)
  | Other.

Definition gas_price (c : SyscallCat) : Z :=
  match c with
  | StorageContext         => 16
  | StorageAccessors       => 32768
  | RuntimePlatform        => 8
  | RuntimeBurnGas         => 16
  | RuntimeCheckWitness    => 1024
  | RuntimeLogLoad         => 32768
  | ContractCall           => 32768
  | ContractGetCallFlags   => 16
  | ContractCreateStd      => 256
  | ContractCreateMultisig => 512
  | ContractNativePersist  => 1024
  | CryptoCheck            => 32768
  | IteratorOps            => 32768
  | Other                  => 0
  end.

(* Key economic invariant: every syscall gas price is non-negative. *)
Theorem gas_prices_non_negative : forall c : SyscallCat, 0 <= gas_price c.
Proof.
  intros c; destruct c; simpl; lia.
Qed.

(* Concrete ties to the Rust `1 << N` constants. *)
Example storage_context_price   : gas_price StorageContext         = 2 ^ 4.  Proof. reflexivity. Qed.
Example storage_accessors_price : gas_price StorageAccessors       = 2 ^ 15. Proof. reflexivity. Qed.
Example check_witness_price     : gas_price RuntimeCheckWitness    = 2 ^ 10. Proof. reflexivity. Qed.
Example create_std_price        : gas_price ContractCreateStd      = 2 ^ 8.  Proof. reflexivity. Qed.
Example create_multisig_price   : gas_price ContractCreateMultisig = 2 ^ 9.  Proof. reflexivity. Qed.

(* Storage contexts are cheap (1<<4); storage read/write accessors are the 1<<15 tier. *)
Example storage_relation : gas_price StorageContext < gas_price StorageAccessors.
Proof. reflexivity. Qed.
