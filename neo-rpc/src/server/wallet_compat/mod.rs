//! # neo-rpc::server::wallet_compat
//!
//! Wallet compatibility helpers for RPC responses.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `accounts`: wallet account compatibility helpers.
//! - `errors`: wallet compatibility error vocabulary.
//! - `network_fee`: network-fee estimation helpers.
//! - `probes`: wallet compatibility probe helpers.
//! - `signing`: wallet signing compatibility helpers.
//! - `transaction_builder`: wallet transaction builder helpers.

mod accounts;
mod errors;
mod network_fee;
mod probes;
mod signing;
mod transaction_builder;

use errors::{WalletCompatResult, core_err};

pub(crate) use errors::WalletCompatError;
pub(crate) use network_fee::calculate_network_fee;
pub(crate) use probes::gas_balance_of;
pub(crate) use signing::sign_transaction_with_key;
pub(crate) use transaction_builder::{make_transaction, make_transfer_transaction};
