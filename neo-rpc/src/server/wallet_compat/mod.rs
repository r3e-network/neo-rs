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
//! - `network_fee`: network-fee estimation helpers.
//! - `probes`: wallet compatibility probe helpers.
//! - `transaction_builder`: wallet transaction builder helpers.

use neo_error::{CoreError, CoreResult};
use neo_payloads::transaction::Transaction;
use neo_wallets::KeyPair;

mod accounts;
mod network_fee;
mod probes;
mod transaction_builder;

pub(crate) use network_fee::calculate_network_fee;
pub(crate) use probes::gas_balance_of;
pub(crate) use transaction_builder::{make_transaction, make_transfer_transaction};

/// Wallet-layer failure vocabulary mirroring the C# exceptions the RPC
/// server maps onto JSON-RPC errors.
#[derive(Debug)]
pub(crate) enum WalletCompatError {
    /// C# `InvalidOperationException("Insufficient GAS...")` — wallet
    /// balances cannot cover the system + network fees, or a transfer
    /// amount exceeds the wallet balance.
    InsufficientFunds(String),
    /// Any other invalid-operation failure (faulted probe scripts,
    /// missing contracts, …).
    Other(String),
}

impl std::fmt::Display for WalletCompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientFunds(msg) | Self::Other(msg) => f.write_str(msg),
        }
    }
}

type WalletCompatResult<T> = Result<T, WalletCompatError>;

/// C# `Neo.Wallets.Helper.Sign(IVerifiable, KeyPair, network)`: signs
/// the verifiable's network-prefixed sign data with the key.
pub(crate) fn sign_transaction_with_key(
    tx: &Transaction,
    key: &KeyPair,
    network: u32,
) -> CoreResult<Vec<u8>> {
    let data = neo_payloads::get_sign_data(tx, network)
        .map_err(|err| CoreError::other(err.to_string()))?;
    key.sign(&data)
        .map_err(|err| CoreError::other(err.to_string()))
}

fn core_err(err: neo_error::CoreError) -> WalletCompatError {
    WalletCompatError::Other(err.to_string())
}
