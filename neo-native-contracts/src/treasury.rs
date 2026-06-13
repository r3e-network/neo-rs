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

/// The Treasury native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Treasury;

impl Treasury {
    /// Stable native contract id (matches C# `Treasury`).
    pub const ID: i32 = -11;
    /// Stable native contract name (matches C# `Treasury.Name`).
    pub const NAME: &'static str = "Treasury";

    /// Construct a new `Treasury` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the Treasury script hash.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the Treasury script hash.
    pub fn script_hash() -> UInt160 {
        *TREASURY_HASH
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
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    // C# `Treasury.Activations => [Hardfork.HF_Faun]` (Treasury.cs:29): the
    // contract does not exist before HF_Faun. Without this override Treasury
    // would be genesis-active in neo-rs, diverging native deployment and
    // manifest state below the configured Faun height. If a custom/private
    // config omits Faun, C# `IsActive` treats ActiveIn as genesis-active, while
    // `IsInitializeBlock` skips the missing hardfork initialization block.
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfFaun)
    }

    /// C# `Treasury.OnManifestCompose` (Treasury.cs:31-34): unconditional —
    /// the contract only exists from HF_Faun onwards.
    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
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
                let authorized =
                    crate::committee::is_committee_witness(engine, "Treasury::verify")?;
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
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["onNEP17Payment", "onNEP11Payment", "verify"]);
        // Payment callbacks: RequiredCallFlags None, Void return, and
        // manifest-SAFE — C# derives Safe = (None & ~CallFlags.ReadOnly) == 0
        // (ContractMethodMetadata.cs:74).
        assert!(
            c.methods()
                .iter()
                .filter(|m| m.name != "verify")
                .all(|m| m.safe
                    && m.required_call_flags == 0
                    && m.return_type == ContractParameterType::Void)
        );
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
        // Neo N3 v3.10.0 MainNet schedules Faun at 8,800,000, so default
        // settings must not expose Treasury before that block.
        let settings = ProtocolSettings::default();
        assert!(!c.is_active(&settings, 8_799_999));
        assert!(c.is_active(&settings, 8_800_000));
        assert!(c.is_active(&settings, u32::MAX));

        // C# `IsActive` (NativeContract.cs) falls back to `activeIn = 0` for an
        // unconfigured ActiveIn hardfork, which keeps custom/private configs
        // that omit Faun genesis-active.
        let mut omitted = ProtocolSettings::default();
        omitted.hardforks.remove(&Hardfork::HfFaun);
        assert!(c.is_active(&omitted, 0));

        let mut custom = ProtocolSettings::default();
        custom.hardforks.insert(Hardfork::HfFaun, 10);
        assert!(!c.is_active(&custom, 9));
        assert!(c.is_active(&custom, 10));

        let state = build_native_contract_state(&c, &custom, 10);
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
    use neo_payloads::{Block, Header};
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_storage::persistence::DataCache;
    use neo_vm_rs::VmState;
    use crate::test_support::{
        committee_address, deploy_native, hex, sample_committee, seed_committee,
        CM_PREFIX_CONTRACT, NEO_PREFIX_COMMITTEE,
    };

    /// Runs `Treasury::verify()` via System.Contract.Call, signed (Global) by
    /// `signer`. Returns the final VM state and the boolean result.
    fn call_verify(
        snapshot: Arc<DataCache>,
        signer: UInt160,
        settings: ProtocolSettings,
        block_height: u32,
    ) -> (VmState, Option<bool>) {
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

        let mut header = Header::new();
        header.set_index(block_height);
        let block = Block::from_parts(header, Vec::new());

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            Some(block),
            settings,
            10_000_000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        let top = engine
            .result_stack()
            .peek(0)
            .ok()
            .and_then(|item| item.as_bool().ok());
        (state, top)
    }

    #[test]
    fn verify_is_true_only_with_the_committee_witness() {
        crate::install();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        // Deploy Treasury directly so this test can focus on verify witness
        // behavior; activation boundaries are covered separately.
        let settings = ProtocolSettings::default();
        let faun_height = settings
            .hardforks
            .get(&Hardfork::HfFaun)
            .copied()
            .expect("default settings schedule Faun");
        deploy_native(
            &cache,
            &build_native_contract_state(&Treasury, &settings, faun_height),
        );
        let snapshot = Arc::new(cache);

        // Signed by the committee multisig address -> true.
        let (state, result) = call_verify(
            Arc::clone(&snapshot),
            committee_address(&committee),
            settings.clone(),
            faun_height,
        );
        assert_eq!(state, VmState::HALT, "verify must HALT");
        assert_eq!(result, Some(true), "the committee witness verifies");

        // Signed by an unrelated account -> false (a clean HALT, no fault).
        let stranger = UInt160::from_bytes(&[0x21; 20]).unwrap();
        let (state, result) = call_verify(
            Arc::clone(&snapshot),
            stranger,
            settings.clone(),
            faun_height,
        );
        assert_eq!(state, VmState::HALT, "verify must HALT");
        assert_eq!(result, Some(false), "a non-committee witness fails");
    }
}
