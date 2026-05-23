use super::OracleContract;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl OracleContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        let methods = vec![
            NativeMethod::unsafe_method(
                "request".to_string(),
                0,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::Any,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            ),
            NativeMethod::safe(
                "getPrice".to_string(),
                1 << 15,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::unsafe_method(
                "setPrice".to_string(),
                1 << 15,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            ),
            NativeMethod::unsafe_method(
                "finish".to_string(),
                0,
                (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits(),
                Vec::new(),
                ContractParameterType::Void,
            ),
            NativeMethod::safe(
                "verify".to_string(),
                1 << 15,
                Vec::new(),
                ContractParameterType::Boolean,
            ),
        ];
        methods
            .into_iter()
            .map(|method| match method.name.as_str() {
                "request" => method.with_parameter_names(vec![
                    "url".to_string(),
                    "filter".to_string(),
                    "callback".to_string(),
                    "userData".to_string(),
                    "gasForResponse".to_string(),
                ]),
                "setPrice" => method.with_parameter_names(vec!["price".to_string()]),
                _ => method,
            })
            .collect()
    }

    pub(super) fn event_descriptors() -> Vec<ContractEventDescriptor> {
        vec![
            ContractEventDescriptor::new(
                "OracleRequest".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Id".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("OracleRequest.Id"),
                    ContractParameterDefinition::new(
                        "RequestContract".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("OracleRequest.RequestContract"),
                    ContractParameterDefinition::new(
                        "Url".to_string(),
                        ContractParameterType::String,
                    )
                    .expect("OracleRequest.Url"),
                    ContractParameterDefinition::new(
                        "Filter".to_string(),
                        ContractParameterType::String,
                    )
                    .expect("OracleRequest.Filter"),
                ],
            )
            .expect("OracleRequest event descriptor"),
            ContractEventDescriptor::new(
                "OracleResponse".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "Id".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("OracleResponse.Id"),
                    ContractParameterDefinition::new(
                        "OriginalTx".to_string(),
                        ContractParameterType::Hash256,
                    )
                    .expect("OracleResponse.OriginalTx"),
                ],
            )
            .expect("OracleResponse event descriptor"),
        ]
    }
}
