use std::sync::LazyLock;

use neo_execution::NativeMethod;
use neo_primitives::{CallFlags, ContractParameterType};

pub(super) static LEDGER_CONTRACT_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "currentHash",
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Hash256,
        ),
        NativeMethod::new(
            "currentIndex",
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "getTransactionHeight",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Integer,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getTransactionVMState",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Integer,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getTransaction",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Array,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getTransactionSigners",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Array,
        )
        .with_parameter_names(["hash"]),
        // getBlock(indexOrHash: ByteArray) -> Array (TrimmedBlock) | Null.
        NativeMethod::new(
            "getBlock",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::ByteArray],
            ContractParameterType::Array,
        )
        .with_parameter_names(["indexOrHash"]),
        // getTransactionFromBlock(blockIndexOrHash: ByteArray, txIndex: Integer)
        // -> Array (Transaction) | Null. C# CpuFee is 1 << 16 (heavier than the
        // other ledger reads because it loads a whole trimmed block).
        NativeMethod::new(
            "getTransactionFromBlock",
            1 << 16,
            true,
            read_states,
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Array,
        )
        .with_parameter_names(["blockIndexOrHash", "txIndex"]),
    ]
});
