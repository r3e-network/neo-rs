//! Token tracker for NEP-11 and NEP-17 balance/transfer indexing.
//!
//! This module provides balance and transfer history tracking for Neo N3
//! token standards. It indexes Transfer events from blocks and stores
//! balance/transfer records in a separate database.
//!
//! # Architecture
//!
//! The tracker is designed to work as an integrated component (not a plugin):
//! - `TokensTracker` implements `ICommittingHandler` and `ICommittedHandler`
//! - Trackers index data during block commit events
//! - RPC handlers query the indexed data
//!
//! # Supported Standards
//!
//! - **NEP-17**: Fungible tokens (balances, transfer history)
//! - **NEP-11**: Non-fungible tokens (NFT ownership, transfer history)

pub mod extensions;
pub mod runtime;
pub mod service;
pub mod settings;
pub mod trackers;

pub use extensions::{bigint_var_size, bytes_var_size, find_prefix, find_range, to_base64};
pub use runtime::TokensTracker;
pub use service::TokensTrackerService;
pub use settings::TokensTrackerSettings;
pub use trackers::{
    nep_11::{Nep11BalanceKey, Nep11Tracker, Nep11TransferKey},
    nep_17::{Nep17BalanceKey, Nep17Tracker, Nep17TransferKey},
    token_balance::TokenBalance,
    token_transfer::TokenTransfer,
    token_transfer_key::TokenTransferKey,
    tracker_base::{TokenTransferKeyView, Tracker, TrackerBase, TransferRecord},
};
