//! Token tracker plugin (merged from the standalone `neo-tokens-tracker` crate).
//!
//! Exposes NEP-11 and NEP-17 balance/transfer indexing. Lives under
//! `neo_rpc::plugins::tokens_tracker`; enabled by the `server` feature.
//!
//! Original C# reference: `RpcServer.Plugins.TokensTracker`.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod extensions;
pub mod runtime;
pub mod service;
pub mod settings;
pub mod trackers;

pub use extensions::{bigint_var_size, find_prefix, find_range, to_base64};
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
