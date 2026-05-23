use super::OracleContract;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl OracleContract {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            unsafe "request", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [String, String, String, Any, Integer], returns = Void, names = ["url", "filter", "callback", "userData", "gasForResponse"];
            safe "getPrice", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            unsafe "setPrice", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["price"];
            unsafe "finish", fee = 0, flags = [STATES, ALLOW_CALL, ALLOW_NOTIFY], params = [], returns = Void;
            safe "verify", fee = 1 << 15, flags = [], params = [], returns = Boolean;
        ]
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
