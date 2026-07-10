use std::sync::LazyLock;

use neo_execution::NativeMethod;
use neo_primitives::{CallFlags, ContractParameterType};

use super::LedgerContract;
use crate::support::invoke::{NativeMethodBinding, method_metadata};

pub(super) fn ledger_contract_method_bindings<P, D, B>()
-> Vec<NativeMethodBinding<LedgerContract, P, D, B>>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethodBinding::new(
            NativeMethod::new(
                "currentHash",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Hash256,
            ),
            LedgerContract::invoke_current_hash,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "currentIndex",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            ),
            LedgerContract::invoke_current_index,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getTransactionHeight",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Integer,
            )
            .with_parameter_names(["hash"]),
            LedgerContract::invoke_get_transaction_height,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getTransactionVMState",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Integer,
            )
            .with_parameter_names(["hash"]),
            LedgerContract::invoke_get_transaction_vm_state,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getTransaction",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Array,
            )
            .with_parameter_names(["hash"]),
            LedgerContract::invoke_get_transaction,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getTransactionSigners",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::Hash256],
                ContractParameterType::Array,
            )
            .with_parameter_names(["hash"]),
            LedgerContract::invoke_get_transaction_signers,
        ),
        // getBlock(indexOrHash: ByteArray) -> Array (TrimmedBlock) | Null.
        NativeMethodBinding::new(
            NativeMethod::new(
                "getBlock",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Array,
            )
            .with_parameter_names(["indexOrHash"]),
            LedgerContract::invoke_get_block,
        ),
        // getTransactionFromBlock(blockIndexOrHash: ByteArray, txIndex: Integer)
        // -> Array (Transaction) | Null. C# CpuFee is 1 << 16 (heavier than the
        // other ledger reads because it loads a whole trimmed block).
        NativeMethodBinding::new(
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
            LedgerContract::invoke_get_transaction_from_block,
        ),
    ]
}

pub(super) static LEDGER_CONTRACT_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    method_metadata(&ledger_contract_method_bindings::<
        neo_execution::native_contract_provider::NoNativeContractProvider,
        neo_execution::NoDiagnostic,
        neo_storage::EmptyCacheBacking,
    >())
});
