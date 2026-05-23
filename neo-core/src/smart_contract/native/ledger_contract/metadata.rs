use super::LedgerContract;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl LedgerContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        vec![
            NativeMethod::new(
                "currentHash".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                Vec::new(),
                ContractParameterType::Hash256,
            ),
            NativeMethod::new(
                "currentIndex".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                Vec::new(),
                ContractParameterType::Integer,
            ),
            NativeMethod::new(
                "getBlock".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["indexOrHash".to_string()]),
            NativeMethod::new(
                "getTransaction".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash256],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["hash".to_string()]),
            NativeMethod::new(
                "getTransactionFromBlock".to_string(),
                1 << 16,
                true,
                CallFlags::READ_STATES.bits(),
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["blockIndexOrHash".to_string(), "txIndex".to_string()]),
            NativeMethod::new(
                "getTransactionHeight".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash256],
                ContractParameterType::Integer,
            )
            .with_parameter_names(vec!["hash".to_string()]),
            NativeMethod::new(
                "getTransactionSigners".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash256],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["hash".to_string()]),
            NativeMethod::new(
                "getTransactionVMState".to_string(),
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![ContractParameterType::Hash256],
                ContractParameterType::Integer,
            )
            .with_parameter_names(vec!["hash".to_string()]),
        ]
    }
}
