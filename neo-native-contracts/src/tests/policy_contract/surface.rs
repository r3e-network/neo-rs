use super::*;

#[test]
fn native_contract_surface() {
    let c = PolicyContract::new();
    let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "getFeePerByte",
            "getStoragePrice",
            "setFeePerByte",
            "setStoragePrice",
            "getExecFeeFactor",
            "getExecPicoFeeFactor",
            "setExecFeeFactor",
            "getAttributeFee",
            "getAttributeFee",
            "setAttributeFee",
            "setAttributeFee",
            "getBlockedAccounts",
            "setMillisecondsPerBlock",
            "setMaxValidUntilBlockIncrement",
            "setMaxTraceableBlocks",
            "isBlocked",
            "unblockAccount",
            "getMillisecondsPerBlock",
            "getMaxValidUntilBlockIncrement",
            "getMaxTraceableBlocks",
            "blockAccount",
            "blockAccount",
            "setWhitelistFeeContract",
            "removeWhitelistFeeContract",
            "getWhitelistFeeContracts",
            "recoverFund"
        ]
    );
    // The Echidna-era chain-parameter getters are hardfork-gated.
    let mtb = c
        .methods()
        .iter()
        .find(|m| m.name == "getMaxTraceableBlocks")
        .unwrap();
    assert_eq!(mtb.active_in, Some(Hardfork::HfEchidna));
    // unblockAccount is a non-safe, write-flagged (States), Boolean writer.
    let unblock = c
        .methods()
        .iter()
        .find(|m| m.name == "unblockAccount")
        .unwrap();
    assert!(!unblock.safe);
    assert_eq!(unblock.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(unblock.parameters, vec![ContractParameterType::Hash160]);
    assert_eq!(unblock.return_type, ContractParameterType::Boolean);
    // The fee/price setters are non-safe, write-flagged (States), Void methods.
    for name in ["setFeePerByte", "setStoragePrice"] {
        let setter = c.methods().iter().find(|m| m.name == name).unwrap();
        assert!(!setter.safe, "{name} must not be safe");
        assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(setter.return_type, ContractParameterType::Void);
    }
    // The Echidna setter additionally emits a notification (States|AllowNotify).
    let ms = c
        .methods()
        .iter()
        .find(|m| m.name == "setMillisecondsPerBlock")
        .unwrap();
    assert!(!ms.safe);
    assert_eq!(
        ms.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert_eq!(ms.return_type, ContractParameterType::Void);
    assert_eq!(ms.active_in, Some(Hardfork::HfEchidna));
    // The cross-validated Echidna setters are non-safe, States, Void, gated.
    for name in ["setMaxValidUntilBlockIncrement", "setMaxTraceableBlocks"] {
        let m = c.methods().iter().find(|m| m.name == name).unwrap();
        assert!(!m.safe, "{name} must not be safe");
        assert_eq!(m.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(m.return_type, ContractParameterType::Void);
        assert_eq!(m.active_in, Some(Hardfork::HfEchidna));
    }
    // getExecFeeFactor is always present; getExecPicoFeeFactor is HF_Faun-gated;
    // both are safe Integer reads.
    let exec = c
        .methods()
        .iter()
        .find(|m| m.name == "getExecFeeFactor")
        .unwrap();
    assert!(exec.safe && exec.active_in.is_none());
    assert_eq!(exec.return_type, ContractParameterType::Integer);
    assert_eq!(exec.cpu_fee, 1 << 15);
    let pico = c
        .methods()
        .iter()
        .find(|m| m.name == "getExecPicoFeeFactor")
        .unwrap();
    assert!(pico.safe);
    assert_eq!(pico.active_in, Some(Hardfork::HfFaun));
    assert_eq!(pico.return_type, ContractParameterType::Integer);
    // setExecFeeFactor is a non-safe, States, Integer -> Void writer.
    let set_exec = c
        .methods()
        .iter()
        .find(|m| m.name == "setExecFeeFactor")
        .unwrap();
    assert!(!set_exec.safe);
    assert_eq!(set_exec.required_call_flags, CallFlags::STATES.bits());
    assert_eq!(set_exec.parameters, vec![ContractParameterType::Integer]);
    assert_eq!(set_exec.return_type, ContractParameterType::Void);
    assert!(set_exec.active_in.is_none());
    // getAttributeFee/setAttributeFee are dual C# registrations around
    // HF_Echidna. The ABI shape is unchanged, but exactly one descriptor is
    // active at a given height.
    let get_af_versions: Vec<&NativeMethod> = c
        .methods()
        .iter()
        .filter(|m| m.name == "getAttributeFee")
        .collect();
    assert_eq!(get_af_versions.len(), 2);
    for m in &get_af_versions {
        assert!(m.safe);
        assert_eq!(m.cpu_fee, 1 << 15);
        assert_eq!(m.required_call_flags, CallFlags::READ_STATES.bits());
        assert_eq!(m.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(m.return_type, ContractParameterType::Integer);
    }
    assert_eq!(get_af_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
    assert_eq!(get_af_versions[1].active_in, Some(Hardfork::HfEchidna));

    let set_af_versions: Vec<&NativeMethod> = c
        .methods()
        .iter()
        .filter(|m| m.name == "setAttributeFee")
        .collect();
    assert_eq!(set_af_versions.len(), 2);
    for m in &set_af_versions {
        assert!(!m.safe);
        assert_eq!(m.cpu_fee, 1 << 15);
        assert_eq!(m.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(
            m.parameters,
            vec![
                ContractParameterType::Integer,
                ContractParameterType::Integer
            ]
        );
        assert_eq!(m.return_type, ContractParameterType::Void);
    }
    assert_eq!(set_af_versions[0].deprecated_in, Some(Hardfork::HfEchidna));
    assert_eq!(set_af_versions[1].active_in, Some(Hardfork::HfEchidna));
    // getBlockedAccounts is an HF_Faun-gated, safe, no-arg iterator reader.
    let blocked = c
        .methods()
        .iter()
        .find(|m| m.name == "getBlockedAccounts")
        .unwrap();
    assert_eq!(blocked.active_in, Some(Hardfork::HfFaun));
    assert!(blocked.safe && blocked.parameters.is_empty());
    assert_eq!(blocked.return_type, ContractParameterType::InteropInterface);
    assert_eq!(blocked.required_call_flags, CallFlags::READ_STATES.bits());
    // blockAccount is registered twice (C# V0/V1): V0 genesis-active and
    // DeprecatedIn HF_Faun with States; V1 ActiveIn HF_Faun with
    // States|AllowNotify. Both Hash160 -> Boolean, not safe, CpuFee 1<<15.
    let block_versions: Vec<&NativeMethod> = c
        .methods()
        .iter()
        .filter(|m| m.name == "blockAccount")
        .collect();
    assert_eq!(block_versions.len(), 2);
    for m in &block_versions {
        assert!(!m.safe);
        assert_eq!(m.cpu_fee, 1 << 15);
        assert_eq!(m.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(m.return_type, ContractParameterType::Boolean);
    }
    let v0 = block_versions
        .iter()
        .find(|m| m.deprecated_in == Some(Hardfork::HfFaun))
        .expect("blockAccount V0");
    assert_eq!(v0.active_in, None);
    assert_eq!(v0.required_call_flags, CallFlags::STATES.bits());
    let v1 = block_versions
        .iter()
        .find(|m| m.active_in == Some(Hardfork::HfFaun))
        .expect("blockAccount V1");
    assert_eq!(v1.deprecated_in, None);
    assert_eq!(
        v1.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    // Whitelist writers: HF_Faun, not safe, States|AllowNotify, Void.
    let set_wl = c
        .methods()
        .iter()
        .find(|m| m.name == "setWhitelistFeeContract")
        .unwrap();
    assert!(!set_wl.safe);
    assert_eq!(set_wl.active_in, Some(Hardfork::HfFaun));
    assert_eq!(
        set_wl.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert_eq!(
        set_wl.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::String,
            ContractParameterType::Integer,
            ContractParameterType::Integer
        ]
    );
    assert_eq!(set_wl.return_type, ContractParameterType::Void);
    let rm_wl = c
        .methods()
        .iter()
        .find(|m| m.name == "removeWhitelistFeeContract")
        .unwrap();
    assert!(!rm_wl.safe);
    assert_eq!(rm_wl.active_in, Some(Hardfork::HfFaun));
    assert_eq!(
        rm_wl.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert_eq!(
        rm_wl.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::String,
            ContractParameterType::Integer
        ]
    );
    assert_eq!(rm_wl.return_type, ContractParameterType::Void);
    // getWhitelistFeeContracts: HF_Faun, safe, no-arg iterator reader.
    let get_wl = c
        .methods()
        .iter()
        .find(|m| m.name == "getWhitelistFeeContracts")
        .unwrap();
    assert_eq!(get_wl.active_in, Some(Hardfork::HfFaun));
    assert!(get_wl.safe && get_wl.parameters.is_empty());
    assert_eq!(get_wl.return_type, ContractParameterType::InteropInterface);
    assert_eq!(get_wl.required_call_flags, CallFlags::READ_STATES.bits());
    // recoverFund: HF_Faun, not safe, States|AllowNotify, two Hash160 args.
    let recover = c
        .methods()
        .iter()
        .find(|m| m.name == "recoverFund")
        .unwrap();
    assert!(!recover.safe);
    assert_eq!(recover.active_in, Some(Hardfork::HfFaun));
    assert_eq!(
        recover.required_call_flags,
        (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
    );
    assert_eq!(
        recover.parameters,
        vec![
            ContractParameterType::Hash160,
            ContractParameterType::Hash160
        ]
    );
    assert_eq!(recover.return_type, ContractParameterType::Boolean);
    assert_eq!(recover.cpu_fee, 1 << 15);
}
