//! Wallet-side transaction building, ported from C# Neo v3.10.0.
//!
//! The C# RPC wallet endpoints lean on `Neo.Wallets.Helper.CalculateNetworkFee`
//! and `Neo.Wallets.Wallet.MakeTransaction`. Those helpers have no
//! counterpart in the `neo-wallets` crate yet, so this module keeps the
//! RPC-facing compatibility facade while the parity-sensitive paths live in
//! focused submodules:
//!
//! - [`calculate_network_fee`] — `Helper.CalculateNetworkFee(tx, snapshot,
//!   settings, accountScript, maxExecutionCost)`.
//! - [`make_transaction`] — `Wallet.MakeTransaction(snapshot, script,
//!   sender, cosigners, attributes, maxGas)`.
//! - [`make_transfer_transaction`] — `Wallet.MakeTransaction(snapshot,
//!   outputs, from, cosigners)`.
//! - [`sign_transaction_with_key`] — `Helper.Sign(verifiable, key, network)`.
//!
//! All engine probes run the real native/contract code through a fresh
//! [`neo_execution::ApplicationEngine`], matching the C# `ApplicationEngine.Run`
//! test invocations these algorithms are specified in terms of.

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
