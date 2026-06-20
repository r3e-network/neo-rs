use super::*;

/// Every Rust `NativeMethod` (raw tables, including hardfork-gated dual
/// registrations that never co-exist in one manifest) must satisfy the C#
/// derivation `Safe = (RequiredCallFlags & ~CallFlags.ReadOnly) == 0`
/// (ContractMethodMetadata.cs:74). C# cannot express a safe flag that
/// disagrees with the method's call flags; this closes the same degree of
/// freedom in the hand-maintained Rust tables.
#[test]
fn native_method_safe_flags_follow_csharp_derivation() {
    let contracts = StandardNativeProvider::new().all_native_contracts();
    for contract in &contracts {
        for method in contract.methods() {
            let derived = method.required_call_flags & !CallFlags::READ_ONLY.bits() == 0;
            assert_eq!(
                method.safe,
                derived,
                "{}::{}/{}: safe={} but RequiredCallFlags={:#04x} derives safe={} \
                 (C# ContractMethodMetadata.cs:74)",
                contract.name(),
                method.name,
                method.parameters.len(),
                method.safe,
                method.required_call_flags,
                derived,
            );
        }
    }
}

/// C# `FungibleToken.Transfer` is inherited by both NEO and GAS and is
/// annotated with `CpuFee = 1 << 17`, `StorageFee = 50`, and
/// `RequiredCallFlags = States | AllowCall | AllowNotify`
/// (`neo_csharp/.../Native/FungibleToken.cs`). The storage fee is charged by
/// `NativeContract.Invoke` before dispatch, so omitting it on one token is a
/// consensus fee divergence.
#[test]
fn fungible_token_transfer_fees_match_csharp_attribute() {
    let required = (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits();
    for (name, contract) in [
        (
            "NeoToken",
            Box::new(NeoToken::new()) as Box<dyn NativeContract>,
        ),
        (
            "GasToken",
            Box::new(GasToken::new()) as Box<dyn NativeContract>,
        ),
    ] {
        let transfer = contract
            .methods()
            .iter()
            .find(|method| method.name == "transfer")
            .expect("transfer method");
        assert_eq!(transfer.cpu_fee, 1 << 17, "{name} transfer CpuFee");
        assert_eq!(transfer.storage_fee, 50, "{name} transfer StorageFee");
        assert_eq!(
            transfer.required_call_flags, required,
            "{name} transfer RequiredCallFlags"
        );
    }
}

/// C# v3.10.0 `PolicyContract.RecoverFund` is
/// `[ContractMethod(Hardfork.HF_Faun, CpuFee = 1 << 15,
/// RequiredCallFlags = CallFlags.States | CallFlags.AllowNotify)]`
/// (`PolicyContract.cs:630`). Requiring `AllowCall` at the invocation gate
/// would be stricter than C# and reject otherwise valid recoveries.
#[test]
fn policy_recover_fund_call_flags_match_csharp_attribute() {
    let recover_fund = PolicyContract::new()
        .methods()
        .iter()
        .find(|method| method.name == "recoverFund")
        .expect("recoverFund method")
        .clone();
    assert_eq!(recover_fund.cpu_fee, 1 << 15);
    assert_eq!(recover_fund.active_in, Some(Hardfork::HfFaun));
    assert_eq!(
        recover_fund.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
}

/// C# v3.10.0 keeps `getAttributeFee` and `setAttributeFee` as pre/post
/// Echidna descriptor pairs: V0 is `DeprecatedIn HF_Echidna`, V1 is
/// `ActiveIn HF_Echidna`. The ABI is unchanged, but the native method metadata
/// is hardfork-gated and feeds the native method cache.
#[test]
fn policy_attribute_fee_method_gates_match_csharp_v3_10() {
    let contract = PolicyContract::new();

    let get_versions: Vec<_> = contract
        .methods()
        .iter()
        .filter(|method| method.name == "getAttributeFee")
        .collect();
    assert_eq!(get_versions.len(), 2);
    assert_eq!(get_versions[0].active_in, None);
    assert_eq!(get_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
    assert_eq!(get_versions[0].cpu_fee, 1 << 15);
    assert_eq!(
        get_versions[0].required_call_flags,
        CallFlags::READ_STATES.bits()
    );
    assert_eq!(get_versions[1].active_in, Some(Hardfork::HfEchidna));
    assert_eq!(get_versions[1].deprecated_in, None);
    assert_eq!(get_versions[1].cpu_fee, 1 << 15);
    assert_eq!(
        get_versions[1].required_call_flags,
        CallFlags::READ_STATES.bits()
    );

    let set_versions: Vec<_> = contract
        .methods()
        .iter()
        .filter(|method| method.name == "setAttributeFee")
        .collect();
    assert_eq!(set_versions.len(), 2);
    assert_eq!(set_versions[0].active_in, None);
    assert_eq!(set_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
    assert_eq!(set_versions[0].cpu_fee, 1 << 15);
    assert_eq!(
        set_versions[0].required_call_flags,
        CallFlags::STATES.bits()
    );
    assert_eq!(set_versions[1].active_in, Some(Hardfork::HfEchidna));
    assert_eq!(set_versions[1].deprecated_in, None);
    assert_eq!(set_versions[1].cpu_fee, 1 << 15);
    assert_eq!(
        set_versions[1].required_call_flags,
        CallFlags::STATES.bits()
    );
}

/// Vendored C# v3.10.0 `CryptoLib.cs` has two `verifyWithECDsa`
/// registrations (genesis V0 deprecated at Cockatrice, Cockatrice V1) and one
/// `verifyWithEd25519` registration (active at Echidna). There is no
/// `HF_Gorgon` native CryptoLib registration in the v3.10.0 reference, so
/// Rust must not refresh CryptoLib or change its verification behavior at
/// Gorgon.
#[test]
fn crypto_lib_signature_method_gates_match_csharp_v3_10() {
    let contract = CryptoLib::new();

    let ed25519: Vec<_> = contract
        .methods()
        .iter()
        .filter(|method| method.name == "verifyWithEd25519")
        .collect();
    assert_eq!(ed25519.len(), 1);
    assert_eq!(ed25519[0].active_in, Some(Hardfork::HfEchidna));
    assert_eq!(ed25519[0].deprecated_in, None);

    let ecdsa: Vec<_> = contract
        .methods()
        .iter()
        .filter(|method| method.name == "verifyWithECDsa")
        .collect();
    assert_eq!(ecdsa.len(), 2);
    assert_eq!(ecdsa[0].active_in, None);
    assert_eq!(ecdsa[0].deprecated_in, Some(Hardfork::HfCockatrice));
    assert_eq!(
        ecdsa[0].parameter_names,
        ["message", "pubkey", "signature", "curve"]
    );
    assert_eq!(ecdsa[1].active_in, Some(Hardfork::HfCockatrice));
    assert_eq!(ecdsa[1].deprecated_in, None);
    assert_eq!(
        ecdsa[1].parameter_names,
        ["message", "pubkey", "signature", "curveHash"]
    );

    assert!(
        !contract.used_hardforks().contains(&Hardfork::HfGorgon),
        "CryptoLib must not gain a Rust-only Gorgon refresh"
    );
}
