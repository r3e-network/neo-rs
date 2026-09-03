//! Hardfork activation contract for native metadata.

use crate::hardfork::Hardfork;

/// Feature or method metadata that is gated by hardfork activation.
pub trait HardforkActivable {
    /// Hardfork where the feature becomes active.
    fn active_in(&self) -> Option<Hardfork>;

    /// Hardfork where the feature becomes deprecated.
    fn deprecated_in(&self) -> Option<Hardfork>;
}
