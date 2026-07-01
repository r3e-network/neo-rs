//! # neo-rpc::plugins::tokens_tracker::trackers
//!
//! Token tracker implementations grouped by token standard.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `nep_11`: NEP-11 token tracking helpers.
//! - `nep_17`: NEP-17 token tracking helpers.
//! - `token_balance`: token balance projection records.
//! - `token_transfer`: token transfer projection records.
//! - `token_transfer_key`: token transfer key records.
//! - `tracker_base`: shared token tracker base behavior.

macro_rules! impl_token_transfer_key_as_ref {
    ($type:ty, $field:tt) => {
        impl AsRef<$crate::plugins::tokens_tracker::trackers::token_transfer_key::TokenTransferKey>
            for $type
        {
            fn as_ref(
                &self,
            ) -> &$crate::plugins::tokens_tracker::trackers::token_transfer_key::TokenTransferKey
            {
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
