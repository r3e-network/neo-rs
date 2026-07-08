//! # neo-rpc::server::rpc_server_wallet
//!
//! Wallet compatibility RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `balance`: wallet balance RPC handlers.
//! - `errors`: Wallet-domain error projection into RPC exceptions.
//! - `ledger_provider`: Ledger read seam used by wallet handlers.
//! - `lifecycle`: wallet open/close, key import/export, and address listing handlers.
//! - `native_provider`: Native-contract read seam used by wallet handlers.
//! - `network_fee`: transaction network-fee estimation handler.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Wallet RPC response construction helpers.
//! - `signing`: C#-compatible wallet signing and relay finalization.
//! - `support`: Shared support helpers that keep domain modules focused.
//! - `transfers`: wallet transfer RPC handlers.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_server::RpcHandler;

mod balance;
mod errors;
mod ledger_provider;
mod lifecycle;
mod native_provider;
mod network_fee;
mod request;
mod response;
mod signing;
mod support;
mod transfers;

/// RPC handler group for wallet management and transfer methods.
pub struct RpcServerWallet;

impl RpcServerWallet {
    /// Registers all wallet RPC handlers.
    ///
    /// # Security
    /// Wallet methods are marked as protected metadata. Authentication is enforced
    /// only when RPC basic auth is configured, matching C# behavior.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            // Wallet methods are marked as protected metadata.
            protected;
            "closewallet" => Self::close_wallet,
            "dumpprivkey" => Self::dump_priv_key,
            "getnewaddress" => Self::get_new_address,
            "getwalletbalance" => Self::get_wallet_balance,
            "getwalletunclaimedgas" => Self::get_wallet_unclaimed_gas,
            "importprivkey" => Self::import_priv_key,
            "listaddress" => Self::list_address,
            "openwallet" => Self::open_wallet,
            "calculatenetworkfee" => Self::calculate_network_fee,
            "sendfrom" => Self::send_from,
            "sendtoaddress" => Self::send_to_address,
            "sendmany" => Self::send_many,
            "canceltransaction" => Self::cancel_transaction,
        ]
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_wallet.rs"]
mod tests;
