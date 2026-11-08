//! GAS token method/event metadata.
//!
//! Migrated from `neo-core/src/smart_contract/native/gas_token/metadata.rs`.
//! Import adaptation: `crate::smart_contract::manifest::*` →
//! `neo_smart_contract::manifest::*`; `crate::smart_contract::native::*` →
//! `crate::native::*`.

use super::GasToken;
use crate::native::metadata_macros::event_descriptor;
use crate::native::{FungibleToken, NativeMethod};
use neo_smart_contract::manifest::ContractEventDescriptor;

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
