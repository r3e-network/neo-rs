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
//! - `lifecycle`: wallet open/close, key import/export, and address listing handlers.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `support`: Shared support helpers that keep domain modules focused.
//! - `transfers`: wallet transfer RPC handlers.
//! - `tests`: Module-local tests and regression coverage.

#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_native_contracts::{LedgerContract, NeoToken};
use neo_primitives::UInt160;
#[cfg(test)]
use neo_primitives::WitnessScope;
#[cfg(test)]
use neo_vm_rs::OpCode;
#[cfg(test)]
use neo_wallets::{KeyPair, Nep6Wallet};
use neo_wallets::{Wallet as CoreWallet, WalletError, WalletResult};
use num_bigint::BigInt;
use num_traits::Zero;
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::runtime::{Builder as RuntimeBuilder, Handle};

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, invalid_params};
use crate::server::rpc_server::{RpcHandler, RpcServer};
use crate::server::wallet_compat;
#[cfg(test)]
use support::signature_contract_pubkey;

mod balance;
mod lifecycle;
mod request;
mod support;
mod transfers;

use self::request::{NetworkFeeRequest, WalletBalanceRequest};

/// RPC handler group for wallet management and transfer methods.
pub struct RpcServerWallet;

const INVALID_OPERATION_HRESULT: i32 = -2146233079;

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

    fn get_wallet_balance(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = WalletBalanceRequest::parse(params)?;
        let wallet = Self::require_wallet(server)?;
        // C# GetWalletBalance sums per-account `balanceOf` script probes
        // (Wallet.GetAvailable). The engine-script path below invokes the
        // same native `balanceOf` / `decimals` methods for every NEP-17
        // asset, NEO and GAS included.
        let balance = Self::calculate_nep17_balance(server, wallet.as_ref(), &request.asset)?;
        Ok(json!({"balance": balance.to_string()}))
    }

    fn get_wallet_unclaimed_gas(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let height = ledger
            .current_index(store.data_cache())
            .map_err(|err| {
                RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
            })?
            .saturating_add(1);
        let neo_hash = NeoToken::script_hash();
        let snapshot = Arc::new(store.data_cache().clone());
        let mut total = BigInt::zero();
        for account in wallet.accounts() {
            // C# GetWalletUnclaimedGas sums NativeContract.NEO.UnclaimedGas
            // per account; the engine probe invokes the same native
            // `unclaimedGas(account, end)` method.
            let gas = crate::server::native_queries::NativeQueries::neo_unclaimed_gas(
                server,
                Arc::clone(&snapshot),
                &neo_hash,
                &account.script_hash(),
                height,
            )
            .map_err(internal_error)?;
            total += gas;
        }
        Ok(Value::String(total.to_string()))
    }

    fn calculate_network_fee(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = NetworkFeeRequest::parse(params)?;
        let system = server.system();
        let store = system.store_cache();
        let settings = system.settings();
        let native_contract_provider = system.native_contract_provider();
        let wallet = server.wallet();
        let account_script = |hash: &UInt160| -> Option<Vec<u8>> {
            wallet.as_ref().and_then(|wallet| {
                wallet
                    .account(hash)
                    .and_then(|account| account.contract().map(|contract| contract.script.clone()))
            })
        };
        let fee = wallet_compat::calculate_network_fee(
            &request.transaction,
            store.data_cache(),
            &settings,
            &native_contract_provider,
            &account_script,
            server.settings().max_gas_invoke,
        )
        .map_err(|err| invalid_params(err.to_string()))?;
        Ok(json!({"networkfee": fee.to_string()}))
    }

    fn parse_script_hash(server: &RpcServer, value: &str) -> Result<UInt160, RpcException> {
        let version = server.system().settings().address_version;
        request::parse_wallet_script_hash(value, version)
    }

    fn await_wallet_future<T: Send + 'static>(
        future: Pin<Box<dyn Future<Output = WalletResult<T>> + Send>>,
    ) -> Result<T, RpcException> {
        // The RPC handlers are synchronous, so we must block on the wallet
        // future here. When a tokio runtime is available we always use
        // `block_in_place`, which is safe on a multi-thread runtime.
        //
        // The previous code spawned a fresh `CurrentThread` runtime when the
        // host was a current-thread runtime. That path could silently deadlock:
        // if the wallet future depended on the parent runtime's reactor (e.g.
        // a `tokio::time::Sleep` or an mpsc receiver), the parent thread would
        // block waiting for the spawned runtime, which in turn could never
        // drive the parent's resources.
        //
        // `block_in_place` panics on a current-thread runtime — but that is a
        // loud, immediate failure that tells the operator to configure the RPC
        // server with a multi-thread runtime, rather than a silent hang.
        let result = if let Ok(handle) = Handle::try_current() {
            tokio::task::block_in_place(move || handle.block_on(future))
        } else {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
                })?
                .block_on(future)
        };
        result.map_err(Self::wallet_failure)
    }

    fn save_wallet(wallet: &Arc<dyn CoreWallet>) -> Result<(), RpcException> {
        let wallet_clone = Arc::clone(wallet);
        Self::await_wallet_future(Box::pin(async move { wallet_clone.save().await }))
    }

    fn wallet_compat_failure(err: wallet_compat::WalletCompatError) -> RpcException {
        match err {
            wallet_compat::WalletCompatError::InsufficientFunds(_) => {
                RpcException::from(RpcError::insufficient_funds_wallet())
            }
            wallet_compat::WalletCompatError::Other(message) => {
                RpcException::from(RpcError::wallet_not_supported().with_data(message))
            }
        }
    }

    fn wallet_failure(err: WalletError) -> RpcException {
        match err {
            WalletError::InvalidPassword => {
                RpcException::from(RpcError::wallet_not_supported().with_data("Invalid password."))
            }
            WalletError::WalletFileNotFound(path) => {
                RpcException::from(RpcError::wallet_not_found().with_data(path))
            }
            WalletError::AccountNotFound(hash) => {
                RpcException::from(RpcError::unknown_account().with_data(format!("{hash}")))
            }
            WalletError::InsufficientFunds => {
                RpcException::from(RpcError::insufficient_funds_wallet())
            }
            WalletError::Io(err) => {
                RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
            }
            other => {
                RpcException::from(RpcError::wallet_not_supported().with_data(other.to_string()))
            }
        }
    }

    fn require_wallet(server: &RpcServer) -> Result<Arc<dyn CoreWallet>, RpcException> {
        server
            .wallet()
            .ok_or_else(|| RpcException::from(RpcError::no_opened_wallet()))
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_wallet.rs"]
mod tests;
