use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_execution::{NativeEvent, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType};

use super::{CONTRACT_DEPLOY_EVENT, CONTRACT_DESTROY_EVENT, CONTRACT_UPDATE_EVENT};

pub(super) static CONTRACT_MANAGEMENT_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "getContract",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Array,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getContractById",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Integer],
            ContractParameterType::Array,
        )
        .with_parameter_names(["id"]),
        NativeMethod::new(
            "getMinimumDeploymentFee",
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        // HF_Echidna added the cheap existence check (CpuFee 1<<14).
        NativeMethod::new(
            "isContract",
            1 << 14,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["hash"]),
        // C# HasMethod is ungated; only IsContract is HF_Echidna-gated.
        NativeMethod::new(
            "hasMethod",
            1 << 15,
            true,
            read_states,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["hash", "method", "pcount"]),
        // Committee-gated setter: not safe, States, Integer -> Void.
        NativeMethod::new(
            "setMinimumDeploymentFee",
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["value"]),
        // getContractHashes() -> Iterator over (id, hash) for deployed contracts.
        NativeMethod::new(
            "getContractHashes",
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::InteropInterface,
        ),
        // destroy(): the calling contract destroys itself. Not safe,
        // States|AllowNotify, Void.
        NativeMethod::new(
            "destroy",
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![],
            ContractParameterType::Void,
        ),
        // deploy(nefFile, manifest) / deploy(nefFile, manifest, data): C#
        // [ContractMethod(RequiredCallFlags = CallFlags.States |
        // CallFlags.AllowNotify)] — CpuFee 0 (the deployment fee is charged
        // inside the method body), returns the new ContractState (Array).
        NativeMethod::new(
            "deploy",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
            ],
            ContractParameterType::Array,
        )
        .with_parameter_names(["nefFile", "manifest"]),
        NativeMethod::new(
            "deploy",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
                ContractParameterType::Any,
            ],
            ContractParameterType::Array,
        )
        .with_parameter_names(["nefFile", "manifest", "data"]),
        // update(nefFile?, manifest?) / update(nefFile?, manifest?, data):
        // same C# attribute shape, Void return; the nullable byte-array args
        // arrive through the dispatcher's null mask.
        NativeMethod::new(
            "update",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["nefFile", "manifest"]),
        NativeMethod::new(
            "update",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
                ContractParameterType::Any,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["nefFile", "manifest", "data"]),
    ]
});

/// ContractManagement's `[ContractEvent]` declarations
/// (ContractManagement.cs:40-42), all ungated and all carrying a single
/// `Hash` parameter (capital H — the C# attribute argument).
pub(super) static CONTRACT_MANAGEMENT_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(
            0,
            CONTRACT_DEPLOY_EVENT,
            &[("Hash", ContractParameterType::Hash160)],
        ),
        NativeEvent::new(
            1,
            CONTRACT_UPDATE_EVENT,
            &[("Hash", ContractParameterType::Hash160)],
        ),
        NativeEvent::new(
            2,
            CONTRACT_DESTROY_EVENT,
            &[("Hash", ContractParameterType::Hash160)],
        ),
    ]
});
