use super::Notary;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl Notary {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        vec![
            // Query methods
            NativeMethod::safe(
                "balanceOf".to_string(),
                1 << 15,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::safe(
                "expirationOf".to_string(),
                1 << 15,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::safe(
                "getMaxNotValidBeforeDelta".to_string(),
                1 << 15,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "verify".to_string(),
                1 << 15,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Boolean,
            )
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["signature".to_string()]),
            // Deposit management methods (write operations)
            NativeMethod::unsafe_method(
                "onNEP17Payment".to_string(),
                1 << 15,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec![
                "from".to_string(),
                "amount".to_string(),
                "data".to_string(),
            ]),
            NativeMethod::unsafe_method(
                "lockDepositUntil".to_string(),
                1 << 15,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            )
            .with_parameter_names(vec!["account".to_string(), "till".to_string()]),
            NativeMethod::unsafe_method(
                "withdraw".to_string(),
                1 << 15,
                CallFlags::ALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                ],
                ContractParameterType::Boolean,
            )
            .with_parameter_names(vec!["from".to_string(), "to".to_string()]),
            NativeMethod::unsafe_method(
                "setMaxNotValidBeforeDelta".to_string(),
                1 << 15,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec!["value".to_string()]),
        ]
    }
}
