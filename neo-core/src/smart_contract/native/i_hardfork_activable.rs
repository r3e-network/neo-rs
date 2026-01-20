//! IHardforkActivable - matches C# Neo.SmartContract.Native.IHardforkActivable exactly.

use crate::hardfork::Hardfork;

/// Interface for hardfork-activable features (matches C# IHardforkActivable).
pub trait IHardforkActivable {
    /// Hardfork where the feature becomes active.
    fn active_in(&self) -> Option<Hardfork>;

    /// Hardfork where the feature becomes deprecated.
    fn deprecated_in(&self) -> Option<Hardfork>;
}
