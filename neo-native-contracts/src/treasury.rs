//! Treasury native contract (id -11).
//!
//! Implements the full ABI of the C# `Neo.SmartContract.Native.Treasury`:
//! the NEP-17 / NEP-11 payment callbacks — in C# both `OnNEP17Payment` and
//! `OnNEP11Payment` have empty bodies, the Treasury simply accepts incoming
//! token transfers, so the implementations here are exact no-ops — and
//! `verify`, the committee witness check that gates Treasury-signed
//! transactions (`CheckCommittee(engine)`).

use std::any::Any;
use std::sync::LazyLock;

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};

use crate::hashes::TREASURY_HASH;

/// Lazily-initialised script-hash handle for the Treasury contract.
pub static TREASURY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *TREASURY_HASH);

/// The Treasury native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Treasury;

impl Treasury {
    /// Stable native contract id (matches C# `Treasury`).
    pub const ID: i32 = -11;

    /// Construct a new `Treasury` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the Treasury script hash.
    pub fn script_hash() -> UInt160 {
        *TREASURY_HASH_REF
    }
}

static TREASURY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    use ContractParameterType::{Any as AnyType, ByteArray, Hash160, Integer, Void};
    // C# `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags = CallFlags.None)]`
    // (Treasury.cs OnNEP17Payment/OnNEP11Payment). ContractMethodMetadata
    // derives `Safe = (None & ~CallFlags.ReadOnly) == 0`, so both payment
    // callbacks are manifest-safe (unlike Notary's, which requires States).
    vec![
        NativeMethod::new(
            "onNEP17Payment".to_string(),
            1 << 5,
            true,
            0,
            vec![Hash160, Integer, AnyType],
            Void,
        )
        .with_parameter_names(["from", "amount", "data"]),
        NativeMethod::new(
            "onNEP11Payment".to_string(),
            1 << 5,
            true,
            0,
            vec![Hash160, Integer, ByteArray, AnyType],
            Void,
        )
        .with_parameter_names(["from", "amount", "tokenId", "data"]),
        // C# `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags =
        // CallFlags.ReadStates)] private bool Verify(ApplicationEngine engine)`
        // (Treasury.cs:41-42): ReadStates ⊆ ReadOnly -> manifest-safe.
        NativeMethod::new(
            "verify".to_string(),
            1 << 5,
            true,
            CallFlags::READ_STATES.bits(),
            vec![],
            ContractParameterType::Boolean,
        ),
    ]
});

impl NativeContract for Treasury {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *TREASURY_HASH_REF
    }

    fn name(&self) -> &str {
        "Treasury"
    }

    // C# `Treasury.Activations => [Hardfork.HF_Faun]` (Treasury.cs:29): the
    // contract does not exist before HF_Faun. Without this override Treasury
    // would be genesis-active in neo-rs, diverging native deployment and
    // manifest state below the Faun height (an unscheduled Faun means never
    // active, matching C# `IsActive`).
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfFaun)
    }

    /// C# `Treasury.OnManifestCompose` (Treasury.cs:31-34): unconditional —
    /// the contract only exists from HF_Faun onwards.
    fn supported_standards(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<String> {
        vec![
            "NEP-26".to_string(),
            "NEP-27".to_string(),
            "NEP-30".to_string(),
        ]
    }

    fn methods(&self) -> &[NativeMethod] {
        &TREASURY_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            // Both callbacks are no-ops in C# (empty bodies); they return Void,
            // so an empty payload pushes nothing onto the stack.
            "onNEP17Payment" | "onNEP11Payment" => Ok(Vec::new()),
            // C# `Treasury.Verify` (Treasury.cs:41-42) = `CheckCommittee(engine)`:
            // true iff the committee multi-sig address witnesses the current
            // container — the witness seam for Treasury-signed transactions.
            "verify" => {
                let authorized = engine.check_committee_witness().map_err(|e| {
                    CoreError::invalid_operation(format!("Treasury::verify committee check: {e}"))
                })?;
                Ok(vec![u8::from(authorized)])
            }
            other => Err(CoreError::invalid_operation(format!(
                "Treasury method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = Treasury::new();
        assert_eq!(NativeContract::id(&c), -11);
        assert_eq!(NativeContract::name(&c), "Treasury");
        assert_eq!(NativeContract::hash(&c), *TREASURY_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["onNEP17Payment", "onNEP11Payment", "verify"]);
        // Payment callbacks: RequiredCallFlags None, Void return, and
        // manifest-SAFE — C# derives Safe = (None & ~CallFlags.ReadOnly) == 0
        // (ContractMethodMetadata.cs:74).
        assert!(c
            .methods()
            .iter()
            .filter(|m| m.name != "verify")
            .all(|m| m.safe && m.required_call_flags == 0 && m.return_type == ContractParameterType::Void));
        // verify (Treasury.cs:41-42): CpuFee 1<<5, ReadStates (⊆ ReadOnly ->
        // safe), no parameters, Boolean return.
        let verify = c.methods().iter().find(|m| m.name == "verify").unwrap();
        assert!(verify.safe);
        assert_eq!(verify.cpu_fee, 1 << 5);
        assert_eq!(verify.required_call_flags, CallFlags::READ_STATES.bits());
        assert!(verify.parameters.is_empty());
        assert_eq!(verify.return_type, ContractParameterType::Boolean);
    }

    /// C# `Treasury.Activations => [HF_Faun]` (Treasury.cs:29) and
    /// `OnManifestCompose` (Treasury.cs:31-34): the contract activates at
    /// Faun and its manifest declares NEP-26/NEP-27/NEP-30 unconditionally.
    #[test]
    fn faun_activation_and_manifest_standards() {
        use neo_execution::native_contract::build_native_contract_state;

        let c = Treasury::new();
        assert_eq!(NativeContract::active_in(&c), Some(Hardfork::HfFaun));
        // Unscheduled Faun (the default mainnet/testnet config): ACTIVE from
        // genesis. C# `IsActive` (NativeContract.cs:341) falls back to
        // `activeIn = 0` for an unconfigured ActiveIn hardfork ("treated as
        // enabled from the genesis") — only `IsInitializeBlock` treats the
        // unconfigured hardfork as disabled, so the contract is active but
        // never deployed/initialized.
        assert!(c.is_active(&ProtocolSettings::default(), 0));
        assert!(c.is_active(&ProtocolSettings::default(), u32::MAX));

        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 10);
        assert!(!c.is_active(&settings, 9));
        assert!(c.is_active(&settings, 10));

        let state = build_native_contract_state(&c, &settings, 10);
        assert_eq!(
            state.manifest.supported_standards,
            ["NEP-26", "NEP-27", "NEP-30"]
        );
    }
}

/// End-to-end verification of `verify` through the VM: a script
/// `System.Contract.Call`s Treasury and the boolean result reflects whether
/// the committee multisig address witnesses the transaction (C#
/// `Treasury.Verify` = `CheckCommittee(engine)`).
#[cfg(test)]
mod verify_witness_tests {
    use super::*;
    use std::sync::Arc;

    use neo_crypto::ECPoint;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_script_builder::ScriptBuilder;
    use neo_serialization::BinarySerializer;
    use neo_storage::persistence::DataCache;
    use neo_storage::{StorageItem, StorageKey};
    use neo_vm::StackItem;
    use neo_vm_rs::{ExecutionEngineLimits, VmState};

    /// ContractManagement per-contract storage prefix.
    const CM_PREFIX_CONTRACT: u8 = 8;
    /// C# `NeoToken.Prefix_Committee`.
    const NEO_PREFIX_COMMITTEE: u8 = 14;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn sample_committee() -> Vec<ECPoint> {
        // Three valid secp256r1 public keys (Neo N3 standby validators).
        [
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
            "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
        ]
        .iter()
        .map(|h| ECPoint::from_bytes(&hex(h)).unwrap())
        .collect()
    }

    /// Stores NeoToken's committee cache (Array of `Struct[pubkey, votes]`)
    /// under `Prefix_Committee` so `check_committee_witness` can compute the
    /// committee address.
    fn seed_committee(cache: &DataCache, points: &[ECPoint]) {
        let array = StackItem::from_array(
            points
                .iter()
                .map(|p| {
                    StackItem::from_struct(vec![
                        StackItem::from_byte_string(p.to_bytes()),
                        StackItem::from_int(0),
                    ])
                })
                .collect::<Vec<_>>(),
        );
        let bytes =
            BinarySerializer::serialize(&array, &ExecutionEngineLimits::default()).unwrap();
        cache.add(
            StorageKey::new(crate::NeoToken::ID, vec![NEO_PREFIX_COMMITTEE]),
            StorageItem::from_bytes(bytes),
        );
    }

    /// The `m = n - (n - 1) / 2` committee multisig address (2-of-3 here).
    fn committee_address(points: &[ECPoint]) -> UInt160 {
        let script = neo_redeem_script::multi_sig_redeem_script_from_points(2, points).unwrap();
        UInt160::from_script(&script)
    }

    fn deploy_native(cache: &DataCache, state: &ContractState) {
        let mut key = vec![CM_PREFIX_CONTRACT];
        key.extend_from_slice(&state.hash.to_bytes());
        cache.add(
            StorageKey::new(crate::ContractManagement::ID, key),
            StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
        );
    }

    /// Runs `Treasury::verify()` via System.Contract.Call, signed (Global) by
    /// `signer`. Returns the final VM state and the boolean result.
    fn call_verify(snapshot: Arc<DataCache>, signer: UInt160) -> (VmState, Option<bool>) {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("verify".as_bytes());
        builder.emit_push(&Treasury::script_hash().to_array());
        builder.emit_syscall("System.Contract.Call").expect("call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        let top = engine.result_stack().peek(0).ok().and_then(|item| item.as_bool().ok());
        (state, top)
    }

    #[test]
    fn verify_is_true_only_with_the_committee_witness() {
        crate::install();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        // HF_Faun unscheduled (default settings) -> Treasury active from genesis.
        let settings = ProtocolSettings::default();
        deploy_native(&cache, &build_native_contract_state(&Treasury, &settings, 0));
        let snapshot = Arc::new(cache);

        // Signed by the committee multisig address -> true.
        let (state, result) = call_verify(Arc::clone(&snapshot), committee_address(&committee));
        assert_eq!(state, VmState::HALT, "verify must HALT");
        assert_eq!(result, Some(true), "the committee witness verifies");

        // Signed by an unrelated account -> false (a clean HALT, no fault).
        let stranger = UInt160::from_bytes(&[0x21; 20]).unwrap();
        let (state, result) = call_verify(Arc::clone(&snapshot), stranger);
        assert_eq!(state, VmState::HALT, "verify must HALT");
        assert_eq!(result, Some(false), "a non-committee witness fails");
    }
}
