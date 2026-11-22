//! RPC wallet endpoints mirroring RpcServer.Wallet.cs.

#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::Serializable;
use neo_native_contracts::native::{
    GasToken, LedgerContract, NativeContract, NeoToken, PolicyContract,
};
use neo_primitives::BigDecimal;
use neo_primitives::CallFlags;
use neo_primitives::TriggerType;
use neo_primitives::{UInt160, WitnessScope};
use neo_protocol::payloads::Signer;
use neo_protocol::payloads::conflicts::Conflicts;
use neo_protocol::payloads::transaction::Transaction;
use neo_protocol::payloads::transaction_attribute::TransactionAttribute;
use neo_protocol::witness::Witness;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::helper::Helper as ContractHelper;
use neo_storage::persistence::PersistenceDataCache;
use neo_vm::OpCode;
use neo_vm::ScriptBuilder;
use neo_vm::VmState as VMState;
use neo_wallets::helper::Helper as WalletHelper;
use neo_wallets::signing::{
    FrozenSignIntent, ProtocolContext, SIGNING_SCHEMA_VERSION, SignPayload, SignPurpose,
    SignResponse, SignResult,
};
use neo_wallets::{
    AccountSignature, AuthorizationLimits, ContractSignatureContext, Helper, KeyPair, Nep6Wallet,
    TransactionAuthorizationError, TransactionAuthorizer, TransactionChainQuery, TransferOutput,
    Wallet as CoreWallet, WalletAccount, WalletError, WalletResult,
};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::runtime::{Builder as RuntimeBuilder, Handle, RuntimeFlavor};
use zeroize::Zeroizing;

use crate::rpc_error::RpcError;
use crate::rpc_exception::RpcException;
use crate::rpc_helpers::{
    expect_base64_param_with_decode_message, expect_string_param, internal_error, invalid_params,
    new_correlation_id, parse_uint160, parse_uint256,
};
use crate::rpc_relay;
use crate::rpc_server::{RpcHandler, RpcServer, RpcSignerRuntime};

mod management;
mod prepared;
mod query;
mod signing;
mod transfer;

pub struct RpcServerWallet;

const INVALID_OPERATION_HRESULT: i32 = -2146233079;
const SIGNATURE_INVOCATION_SCRIPT_LENGTH: usize = 66;

impl RpcServerWallet {
    /// Registers all wallet RPC handlers.
    ///
    /// # Security
    /// Wallet methods are protected: the dispatcher invokes them only for a
    /// request that authenticated against configured `RpcUser`/`RpcPass`
    /// credentials, and refuses them when none are configured. This is stricter
    /// than C#, which leaves them open on a server without credentials.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            // Wallet methods are protected; see `routes::handlers::process_request`.
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

    fn parse_script_hash(server: &RpcServer, value: &str) -> Result<UInt160, RpcException> {
        if let Ok(hash) = UInt160::from_str(value) {
            return Ok(hash);
        }
        let version = server.system().settings().address_version;
        WalletHelper::to_script_hash(value, version).map_err(invalid_params)
    }

    fn parse_signers(server: &RpcServer, value: &Value) -> Result<Vec<Signer>, RpcException> {
        let array = value
            .as_array()
            .ok_or_else(|| invalid_params("signers must be an array"))?;
        let mut signers = Vec::with_capacity(array.len());
        for entry in array {
            let addr = entry
                .as_str()
                .ok_or_else(|| invalid_params("signer entries must be strings"))?;
            let hash = Self::parse_script_hash(server, addr)?;
            signers.push(Signer::new(hash, WitnessScope::CALLED_BY_ENTRY));
        }
        Ok(signers)
    }

    fn parse_optional_signers(
        server: &RpcServer,
        params: &[Value],
        index: usize,
    ) -> Result<Option<Vec<Signer>>, RpcException> {
        params
            .get(index)
            .map(|value| Self::parse_signers(server, value))
            .transpose()
    }
    fn await_wallet_future<T: Send + 'static>(
        future: impl Future<Output = WalletResult<T>> + Send + 'static,
    ) -> Result<T, RpcException> {
        let result = if let Ok(handle) = Handle::try_current() {
            match handle.runtime_flavor() {
                RuntimeFlavor::CurrentThread => std::thread::spawn(move || {
                    RuntimeBuilder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|err| WalletError::Other(err.to_string()))?
                        .block_on(future)
                })
                .join()
                .map_err(|_| {
                    RpcException::from(
                        RpcError::internal_server_error()
                            .with_data("wallet runtime thread panicked"),
                    )
                })?,
                RuntimeFlavor::MultiThread => {
                    tokio::task::block_in_place(move || handle.block_on(future))
                }
                _ => tokio::task::block_in_place(move || handle.block_on(future)),
            }
        } else {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| internal_error(err.to_string()))?
                .block_on(future)
        };
        result.map_err(Self::wallet_failure)
    }

    /// Maps an unexpected wallet backend failure to a client-safe error.
    ///
    /// R-10: `WalletError` display strings embed filesystem paths and account
    /// material, so the detail is logged under a correlation identifier and the
    /// client only receives a generic message plus that identifier.
    fn wallet_internal_failure(detail: impl ToString) -> RpcException {
        let detail = detail.to_string();
        let correlation_id = new_correlation_id();
        tracing::error!(
            target: "neo::rpc::wallet",
            correlation_id = %correlation_id,
            detail = %detail,
            "internal wallet error"
        );
        RpcException::from(
            RpcError::wallet_not_supported()
                .with_data(format!("Wallet error. Reference: {correlation_id}")),
        )
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
            WalletError::Io(err) => internal_error(err.to_string()),
            other => Self::wallet_internal_failure(other),
        }
    }
    fn require_wallet(server: &RpcServer) -> Result<Arc<dyn CoreWallet>, RpcException> {
        server
            .wallet()
            .ok_or_else(|| RpcException::from(RpcError::no_opened_wallet()))
    }

    /// Drives an authorizer chain-port future to completion.
    ///
    /// [`LedgerChainQuery`] answers every port call from the snapshot in a
    /// single poll, so a plain executor is enough; see the type documentation.
    fn drive<T>(future: impl Future<Output = T>) -> T {
        futures::executor::block_on(future)
    }
}

#[cfg(test)]
mod tests;
