use super::GasToken;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::metadata_macros::event_descriptor;
use crate::smart_contract::native::{FungibleToken, NativeMethod};

impl GasToken {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        <Self as FungibleToken>::ft_nep17_methods()
    }

    pub(super) fn supported_standards_metadata() -> Vec<String> {
        vec!["NEP-17".to_string()]
    }

    pub(super) fn event_descriptors() -> Vec<ContractEventDescriptor> {
        vec![event_descriptor!(
            "Transfer",
            ["from" => Hash160, "to" => Hash160, "amount" => Integer]
        )]
    }
}
