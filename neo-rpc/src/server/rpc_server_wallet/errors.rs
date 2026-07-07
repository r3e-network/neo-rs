//! Wallet-domain error projection for RPC handlers.
//!
//! Wallet modules use this boundary to keep C#-compatible RPC error codes,
//! messages, and transfer compatibility mappings centralized while handler
//! files stay focused on request execution.

use neo_wallets::WalletError;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::wallet_compat;

pub(super) fn wallet_compat_failure(err: wallet_compat::WalletCompatError) -> RpcException {
    match err {
        wallet_compat::WalletCompatError::InsufficientFunds(_) => {
            RpcException::from(RpcError::insufficient_funds_wallet())
        }
        wallet_compat::WalletCompatError::Other(message) => {
            RpcException::from(RpcError::wallet_not_supported().with_data(message))
        }
    }
}

pub(super) fn wallet_failure(err: WalletError) -> RpcException {
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
        WalletError::InsufficientFunds => RpcException::from(RpcError::insufficient_funds_wallet()),
        WalletError::Io(err) => {
            RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
        }
        other => RpcException::from(RpcError::wallet_not_supported().with_data(other.to_string())),
    }
}

pub(super) fn send_from_transfer_error(err: RpcException) -> RpcException {
    map_insufficient_funds(err, |_| {
        RpcException::from(RpcError::invalid_request().with_data("Can not process this request."))
    })
}

pub(super) fn invalid_operation_transfer_error(err: RpcException) -> RpcException {
    map_insufficient_funds(err, |rpc_error| {
        RpcException::new(super::INVALID_OPERATION_HRESULT, rpc_error.error_message())
    })
}

fn map_insufficient_funds(
    err: RpcException,
    map_insufficient: impl FnOnce(RpcError) -> RpcException,
) -> RpcException {
    let rpc_error: RpcError = err.into();
    if rpc_error.code() == RpcError::insufficient_funds_wallet().code() {
        map_insufficient(rpc_error)
    } else {
        RpcException::from(rpc_error)
    }
}
