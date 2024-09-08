
use neo_sdk::prelude::*;
use neo_sdk::types::Hardfork;

/// Trait for contracts that can be activated or deprecated in specific hardforks
pub trait IHardforkActivable {
    /// The hardfork in which this contract or feature becomes active
    fn active_in(&self) -> Option<Hardfork>;

    /// The hardfork in which this contract or feature becomes deprecated
    fn deprecated_in(&self) -> Option<Hardfork>;
}
