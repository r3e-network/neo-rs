//! Token tracker implementations.
//!
//! This module contains the base tracker infrastructure and
//! standard-specific implementations for NEP-11 and NEP-17.

macro_rules! impl_token_transfer_key_as_ref {
    ($type:ty, $field:tt) => {
        impl AsRef<crate::trackers::token_transfer_key::TokenTransferKey>
            for $type
        {
            fn as_ref(
                &self,
            ) -> &crate::trackers::token_transfer_key::TokenTransferKey {
                &self.$field
            }
        }
    };
}

pub(crate) use impl_token_transfer_key_as_ref;

pub mod nep_11;
pub mod nep_17;
pub mod token_balance;
pub mod token_transfer;
pub mod token_transfer_key;
pub mod tracker_base;

pub use tracker_base::TrackerBase;
