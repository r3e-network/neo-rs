//! # neo-rpc::server::rpc_server_wallet::transaction
//!
//! Wallet transaction construction, fee calculation, signing, and relay.
//!
//! ## Boundary
//!
//! This module owns wallet-facing transaction workflows. Wallet lifecycle,
//! request decoding, ledger providers, and response schemas remain in the
//! parent handler group.
//!
//! ## Contents
//!
//! - `network_fee`: transaction network-fee estimation.
//! - `signing`: witness completion and relay finalization.
//! - `transfers`: send and cancel transaction handlers.

use super::{
    RpcServerWallet, errors, ledger_provider, native_provider, request, response, support,
};

mod network_fee;
mod signing;
mod transfers;
