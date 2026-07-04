//! # neo-rpc::plugins::tokens_tracker
//!
//! Token tracker plugin wiring and index-derived token views.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `extensions`: Extension traits layered over the core IO primitives.
//! - `runtime`: Runtime flags, execution context state, and VM-facing support
//!   types.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `settings`: Protocol settings, hardfork gates, and node configuration
//!   records.
//! - `trackers`: Token tracker implementations grouped by token standard.

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
