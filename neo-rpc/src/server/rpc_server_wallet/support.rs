//! Shared support helpers for wallet RPC handlers.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use neo_execution::helper::Helper as ContractHelper;
use neo_primitives::UInt160;
use neo_wallets::{Wallet as CoreWallet, WalletResult};
use tokio::runtime::{Builder as RuntimeBuilder, Handle};

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

use super::{RpcServerWallet, errors, request};

impl RpcServerWallet {
    pub(super) fn parse_script_hash(
        server: &RpcServer,
        value: &str,
    ) -> Result<UInt160, RpcException> {
        let version = server.system().settings().address_version;
        request::parse_wallet_script_hash(value, version)
    }

    pub(super) fn await_wallet_future<T: Send + 'static>(
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
        result.map_err(errors::wallet_failure)
    }

    pub(super) fn save_wallet(wallet: &Arc<dyn CoreWallet>) -> Result<(), RpcException> {
        let wallet_clone = Arc::clone(wallet);
        Self::await_wallet_future(Box::pin(async move { wallet_clone.save().await }))
    }

    pub(super) fn require_wallet(server: &RpcServer) -> Result<Arc<dyn CoreWallet>, RpcException> {
        server
            .wallet()
            .ok_or_else(|| RpcException::from(RpcError::no_opened_wallet()))
    }
}

pub(super) fn signature_contract_pubkey(script: &[u8]) -> Result<Vec<u8>, RpcException> {
    if !ContractHelper::is_signature_contract(script) {
        return Err(RpcException::from(
            RpcError::invalid_params().with_data("Unsupported contract script for signing"),
        ));
    }

    if script.len() < 35 {
        return Err(RpcException::from(
            RpcError::invalid_params().with_data("Invalid signature contract script"),
        ));
    }

    Ok(script[2..35].to_vec())
}
