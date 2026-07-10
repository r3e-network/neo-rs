//! # neo-native-contracts::tests::treasury
//!
//! Test module grouping Native treasury accounting and fund recovery behavior.
//! coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::*;
use neo_primitives::{CallFlags, ContractParameterType};

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
    assert_eq!(c.active_in(), Some(Hardfork::HfFaun));
    // Neo N3 v3.10.1 MainNet schedules Faun at 8,800,000, so default
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
