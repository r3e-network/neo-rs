use std::sync::LazyLock;

use neo_execution::{NativeEvent, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType};

use super::{ORACLE_REQUEST_EVENT, ORACLE_RESPONSE_EVENT};

pub(super) static ORACLE_CONTRACT_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    vec![
        NativeMethod::new(
            "getPrice",
            1 << 15,
            true,
            CallFlags::READ_STATES.bits(),
            vec![],
            ContractParameterType::Integer,
        ),
        // Committee-gated price setter (not safe, States, Void).
        NativeMethod::new(
            "setPrice",
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["price"]),
        // C# Request: CpuFee 0, States | AllowNotify, Void.
        NativeMethod::new(
            "request",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::Any,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["url", "filter", "callback", "userData", "gasForResponse"]),
        // C# Finish: CpuFee 0, States | AllowCall | AllowNotify, Void.
        NativeMethod::new(
            "finish",
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits(),
            vec![],
            ContractParameterType::Void,
        ),
        // C# Verify: CpuFee 1 << 15, CallFlags.None. The C# manifest marks
        // it Safe because `(None & ~ReadOnly) == 0`.
        NativeMethod::new(
            "verify",
            1 << 15,
            true,
            CallFlags::NONE.bits(),
            vec![],
            ContractParameterType::Boolean,
        ),
    ]
});

/// Oracle's `[ContractEvent]` declarations (OracleContract.cs:46-53), both
/// ungated: `OracleRequest` at order 0, `OracleResponse` at order 1.
pub(super) static ORACLE_CONTRACT_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(
            0,
            ORACLE_REQUEST_EVENT,
            &[
                ("Id", ContractParameterType::Integer),
                ("RequestContract", ContractParameterType::Hash160),
                ("Url", ContractParameterType::String),
                ("Filter", ContractParameterType::String),
            ],
        ),
        NativeEvent::new(
            1,
            ORACLE_RESPONSE_EVENT,
            &[
                ("Id", ContractParameterType::Integer),
                ("OriginalTx", ContractParameterType::Hash256),
            ],
        ),
    ]
});
