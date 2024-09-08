use std::fmt;
use crate::contract::nef::CallFlags;

/// Attribute for contract methods in Neo smart contracts.
#[derive(Clone, Debug)]
pub struct ContractMethodAttribute {
    pub name: String,
    pub required_call_flags: CallFlags,
    pub cpu_fee: i64,
    pub storage_fee: i64,
    pub active_in: Option<Hardfork>,
    pub deprecated_in: Option<Hardfork>,
}

impl fmt::Display for ContractMethodAttribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl ContractMethodAttribute {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            required_call_flags: CallFlags::None,
            cpu_fee: 0,
            storage_fee: 0,
            active_in: None,
            deprecated_in: None,
        }
    }

    pub fn new_with_active(active_in: Hardfork) -> Self {
        Self {
            active_in: Some(active_in),
            ..Self::new()
        }
    }

    pub fn new_with_active_and_deprecated(active_in: Hardfork, deprecated_in: Hardfork) -> Self {
        Self {
            active_in: Some(active_in),
            deprecated_in: Some(deprecated_in),
            ..Self::new()
        }
    }

    pub fn new_deprecated(deprecated_in: Hardfork) -> Result<Self, String> {
        Ok(Self {
            deprecated_in: Some(deprecated_in),
            ..Self::new()
        })
    }
}

impl IHardforkActivable for ContractMethodAttribute {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}
