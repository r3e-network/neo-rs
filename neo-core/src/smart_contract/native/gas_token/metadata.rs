use super::GasToken;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::{FungibleToken, NativeMethod};
use crate::smart_contract::ContractParameterType;

impl GasToken {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        <Self as FungibleToken>::ft_nep17_methods()
    }

    pub(super) fn supported_standards_metadata() -> Vec<String> {
        vec!["NEP-17".to_string()]
    }

    pub(super) fn event_descriptors() -> Vec<ContractEventDescriptor> {
        vec![ContractEventDescriptor::new(
            "Transfer".to_string(),
            vec![
                ContractParameterDefinition::new(
                    "from".to_string(),
                    ContractParameterType::Hash160,
                )
                .expect("Transfer.from"),
                ContractParameterDefinition::new("to".to_string(), ContractParameterType::Hash160)
                    .expect("Transfer.to"),
                ContractParameterDefinition::new(
                    "amount".to_string(),
                    ContractParameterType::Integer,
                )
                .expect("Transfer.amount"),
            ],
        )
        .expect("Transfer event descriptor")]
    }
}
