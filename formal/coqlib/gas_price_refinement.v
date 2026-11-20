(**
  Rust→Coq refinement (Phase-1): syscall gas pricing of neo-rs.

  Single Rust source mirrored (the metadata authority):
    neo-smart-contract/src/application_engine/{contract,runtime,storage,crypto,iterator}.rs
      -> register_*_interops (the `price` argument of `register_host_service`)

  This covers the real syscall base-price tiers (values are the C#/Rust `1 << N`
  costs) and proves the economic invariant that every syscall gas price is
  non-negative. A differential Rust test
  (`registered_gas_prices_match_coq_refinement_model` in
  `neo-smart-contract/src/application_engine/state.rs`) ties these to the live
  registered descriptors.

  The virtual machine intentionally keeps no syscall table of its own, so this
  model must never be mirrored against neo-vm again.

  Note on deferred pricing: `System.Runtime.GetRandom`,
  `System.Crypto.CheckMultisig` and the contract-lifecycle syscalls register a
  zero base price and charge their real cost inside the handler (per-call
  counter cost, `n * CheckSigPrice`, native dispatch). The tiers below model the
  registered base price, which is what the differential test asserts.
*)
From Coq Require Import ZArith Lia.
Open Scope Z_scope.

Inductive SyscallCat :=
  | RuntimeEnv             (* Platform, GetTrigger, GetNetwork, GetAddressVersion,
                              GetTime, GetScriptContainer                  -> 1<<3  = 8     *)
  | RuntimeContextHash     (* GetExecutingScriptHash, GetCallingScriptHash,
                              GetEntryScriptHash, GetInvocationCounter,
                              GasLeft, BurnGas, CurrentSigners             -> 1<<4  = 16    *)
  | RuntimeCheckWitness    (* CheckWitness                                -> 1<<10 = 1024  *)
  | RuntimeNotifications   (* GetNotifications                            -> 1<<12 = 4096  *)
  | RuntimeLogLoad         (* LoadScript, Log, Notify                     -> 1<<15 = 32768 *)
  | RuntimeDeferred        (* GetRandom                                    -> 0             *)
  | StorageContext         (* GetContext, GetReadOnlyContext, AsReadOnly  -> 1<<4  = 16    *)
  | StorageAccessors       (* Get, Find, Put, Delete, Local.*             -> 1<<15 = 32768 *)
  | ContractCall           (* Call                                        -> 1<<15 = 32768 *)
  | ContractGetCallFlags   (* GetCallFlags                                -> 1<<10 = 1024  *)
  | ContractDeferred       (* CallNative, CreateStandardAccount,
                              CreateMultisigAccount, NativeOnPersist,
                              NativePostPersist                           -> 0             *)
  | CryptoCheckSig         (* CheckSig                                    -> 1<<15 = 32768 *)
  | CryptoCheckMultisig    (* CheckMultisig                               -> 0             *)
  | IteratorNext           (* Next                                        -> 1<<15 = 32768 *)
  | IteratorValue          (* Value                                       -> 1<<4  = 16    *)
  | Other.

Definition gas_price (c : SyscallCat) : Z :=
  match c with
  | RuntimeEnv             => 8
  | RuntimeContextHash     => 16
  | RuntimeCheckWitness    => 1024
  | RuntimeNotifications   => 4096
  | RuntimeLogLoad         => 32768
  | RuntimeDeferred        => 0
  | StorageContext         => 16
  | StorageAccessors       => 32768
  | ContractCall           => 32768
  | ContractGetCallFlags   => 1024
  | ContractDeferred       => 0
  | CryptoCheckSig         => 32768
  | CryptoCheckMultisig    => 0
  | IteratorNext           => 32768
  | IteratorValue          => 16
  | Other                  => 0
  end.

(* Key economic invariant: every syscall gas price is non-negative. *)
Theorem gas_prices_non_negative : forall c : SyscallCat, 0 <= gas_price c.
Proof.
  intros c; destruct c; simpl; lia.
Qed.

(* Concrete ties to the Rust `1 << N` constants. *)
Example runtime_env_price        : gas_price RuntimeEnv           = 2 ^ 3.  Proof. reflexivity. Qed.
Example runtime_context_price    : gas_price RuntimeContextHash   = 2 ^ 4.  Proof. reflexivity. Qed.
Example check_witness_price      : gas_price RuntimeCheckWitness  = 2 ^ 10. Proof. reflexivity. Qed.
Example notifications_price      : gas_price RuntimeNotifications = 2 ^ 12. Proof. reflexivity. Qed.
Example storage_context_price    : gas_price StorageContext       = 2 ^ 4.  Proof. reflexivity. Qed.
Example storage_accessors_price  : gas_price StorageAccessors     = 2 ^ 15. Proof. reflexivity. Qed.
Example get_call_flags_price     : gas_price ContractGetCallFlags = 2 ^ 10. Proof. reflexivity. Qed.
Example check_sig_price          : gas_price CryptoCheckSig       = 2 ^ 15. Proof. reflexivity. Qed.
Example iterator_value_price     : gas_price IteratorValue        = 2 ^ 4.  Proof. reflexivity. Qed.

(* Deferred tiers register no base price; the handler charges the real cost. *)
Example get_random_base_is_zero      : gas_price RuntimeDeferred      = 0. Proof. reflexivity. Qed.
Example check_multisig_base_is_zero  : gas_price CryptoCheckMultisig  = 0. Proof. reflexivity. Qed.
Example contract_lifecycle_base_zero : gas_price ContractDeferred     = 0. Proof. reflexivity. Qed.

(* Storage contexts are cheap (1<<4); storage read/write accessors are the 1<<15 tier. *)
Example storage_relation : gas_price StorageContext < gas_price StorageAccessors.
Proof. reflexivity. Qed.
