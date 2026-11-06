//! RPC wallet endpoints mirroring RpcServer.Wallet.cs.

#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_core::smart_contract::contract_parameters_context::ContractParametersContext;
use neo_core::smart_contract::helper::Helper as ContractHelper;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::{
    AssetDescriptor, Helper, KeyPair, Nep6Wallet, TransferOutput, Wallet as CoreWallet,
    WalletAccount, WalletError, WalletResult,
};
use neo_crypto::{ECCurve, ECPoint};
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
use neo_signing::{
    FrozenSignIntent, ProtocolContext, SIGNING_SCHEMA_VERSION, SignPayload, SignPurpose,
    SignResponse,
};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_storage::persistence::DataCache;
use neo_vm::OpCode;
use neo_vm::ScriptBuilder;
use neo_vm::VmState as VMState;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tokio::runtime::{Builder as RuntimeBuilder, Handle, RuntimeFlavor};
use zeroize::Zeroizing;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, expect_string_param, internal_error, invalid_params,
    parse_uint160, parse_uint256,
};
use crate::server::rpc_relay;
use crate::server::rpc_server::{RpcHandler, RpcServer, RpcSignerRuntime};

pub struct RpcServerWallet;

const INVALID_OPERATION_HRESULT: i32 = -2146233079;
const SIGNATURE_INVOCATION_SCRIPT_LENGTH: usize = 66;

#[derive(Debug)]
struct PreparedTransaction {
    transaction: Transaction,
    unsigned_data: Vec<u8>,
}

impl PreparedTransaction {
    fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    fn complete(mut self, witnesses: Vec<Witness>) -> Result<Transaction, RpcException> {
        self.transaction.set_witnesses(witnesses);
        let current_unsigned_data = self
            .transaction
            .try_get_hash_data()
            .map_err(|err| internal_error(err.to_string()))?;
        if current_unsigned_data != self.unsigned_data {
            return Err(internal_error(
                "Transaction hash fields changed after authorization; signatures were discarded"
                    .to_string(),
            ));
        }
        Ok(self.transaction)
    }
}

enum WalletAuthorization {
    Signed(Transaction),
    Pending(Value),
}

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

    fn close_wallet(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        server.set_wallet(None);
        Ok(Value::Bool(true))
    }

    fn dump_priv_key(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let address = expect_string_param(params, 0, "dumpprivkey")?;
        let script_hash = Self::parse_script_hash(server, &address)?;
        let wallet = Self::require_wallet(server)?;
        let account = wallet.get_account(&script_hash).ok_or_else(|| {
            RpcException::from(RpcError::unknown_account().with_data(script_hash.to_string()))
        })?;
        if !account.has_key() {
            return Err(RpcException::from(
                RpcError::unknown_account().with_data(format!("{script_hash} is watch-only")),
            ));
        }
        let wif = account.export_wif().map_err(|err| {
            RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
        })?;
        Ok(Value::String(wif))
    }

    fn get_new_address(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let key_pair = KeyPair::generate().map_err(|err| {
            RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
        })?;
        let wallet_clone = Arc::clone(&wallet);
        let key_bytes = Zeroizing::new(*key_pair.private_key());
        let account = Self::await_wallet_future(Box::pin(async move {
            wallet_clone.create_account(key_bytes.as_ref()).await
        }))?;
        Self::save_wallet(&wallet)?;
        Ok(Value::String(account.address()))
    }

    fn get_wallet_balance(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let asset = parse_uint160(params, 0, "getwalletbalance")?;
        let wallet = Self::require_wallet(server)?;
        if asset == NeoToken::new().hash() {
            let token = NeoToken::new();
            let store = server.system().store_cache();
            let mut total = BigInt::zero();
            for account in wallet.get_accounts() {
                if !account.has_key() {
                    continue;
                }
                let amount = token
                    .balance_of_snapshot(&store, &account.script_hash())
                    .map_err(|err| {
                        RpcException::from(
                            RpcError::internal_server_error().with_data(err.to_string()),
                        )
                    })?;
                total += amount;
            }
            let display = BigDecimal::new(total, token.decimals());
            return Ok(json!({ "balance": display.to_string() }));
        }
        if asset == GasToken::new().hash() {
            let token = GasToken::new();
            let store = server.system().store_cache();
            let mut total = BigInt::zero();
            for account in wallet.get_accounts() {
                if !account.has_key() {
                    continue;
                }
                let amount = token.balance_of_snapshot(&store, &account.script_hash());
                total += amount;
            }
            let display = BigDecimal::new(total, token.decimals());
            return Ok(json!({ "balance": display.to_string() }));
        }

        let balance = Self::calculate_nep17_balance(server, &wallet, &asset)?;
        Ok(json!({ "balance": balance.to_string() }))
    }

    fn get_wallet_unclaimed_gas(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let height = ledger
            .current_index(&store)
            .map_err(|err| {
                RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
            })?
            .saturating_add(1);
        let neo = NeoToken::new();
        let mut total = BigInt::zero();
        for account in wallet.get_accounts() {
            let gas = neo
                .unclaimed_gas(&store, &account.script_hash(), height)
                .map_err(|err| {
                    RpcException::from(RpcError::internal_server_error().with_data(err.to_string()))
                })?;
            total += gas;
        }
        Ok(Value::String(total.to_string()))
    }

    fn import_priv_key(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let privkey = expect_string_param(params, 0, "importprivkey")?;
        KeyPair::from_wif(&privkey).map_err(|err| invalid_params(format!("invalid WIF: {err}")))?;
        let wallet = Self::require_wallet(server)?;
        let wallet_clone = Arc::clone(&wallet);
        let privkey_value = privkey;
        let account = Self::await_wallet_future(Box::pin(async move {
            wallet_clone.import_wif(&privkey_value).await
        }))?;
        Self::save_wallet(&wallet)?;
        Ok(Self::account_to_json(&account))
    }

    fn list_address(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let mut entries = Vec::new();
        for account in wallet.get_accounts() {
            entries.push(Self::account_to_json(&account));
        }
        Ok(Value::Array(entries))
    }

    fn open_wallet(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let path = expect_string_param(params, 0, "openwallet")?;
        let password = expect_string_param(params, 1, "openwallet")?;
        // A08: jail wallet paths; use a single not-found error to avoid path/oracle leaks.
        let resolved = Self::resolve_jailed_wallet_path(server, &path)?;
        let system = server.system();
        let settings = Arc::new(system.settings().clone());
        let wallet = Nep6Wallet::from_file(
            resolved
                .to_str()
                .ok_or_else(|| RpcException::from(RpcError::wallet_not_found()))?,
            &password,
            settings,
        );
        let wallet = match wallet {
            Ok(wallet) => wallet,
            Err(WalletError::InvalidPassword) => {
                return Err(RpcException::from(
                    RpcError::wallet_not_supported().with_data("Invalid password."),
                ));
            }
            Err(WalletError::WalletFileNotFound(_)) => {
                return Err(RpcException::from(RpcError::wallet_not_found()));
            }
            Err(WalletError::Io(ref err)) if err.kind() == ErrorKind::NotFound => {
                return Err(RpcException::from(RpcError::wallet_not_found()));
            }
            Err(err) => {
                return Err(RpcException::from(
                    RpcError::wallet_not_supported().with_data(err.to_string()),
                ));
            }
        };
        let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
        server.set_wallet(Some(wallet_arc));
        Ok(Value::Bool(true))
    }

    /// Resolves `openwallet` paths under `WalletDirectory` (or CWD when empty).
    fn resolve_jailed_wallet_path(server: &RpcServer, path: &str) -> Result<PathBuf, RpcException> {
        let not_found = || RpcException::from(RpcError::wallet_not_found());
        let requested = Path::new(path);
        if requested
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(not_found());
        }

        let jail_root = {
            let configured = server.settings().wallet_directory.trim();
            if configured.is_empty() {
                std::env::current_dir().map_err(|_| not_found())?
            } else {
                PathBuf::from(configured)
            }
        };
        let jail = jail_root
            .canonicalize()
            .unwrap_or_else(|_| jail_root.clone());

        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            jail.join(requested)
        };

        let canonical = candidate.canonicalize().map_err(|_| not_found())?;
        if !canonical.starts_with(&jail) {
            return Err(not_found());
        }
        Ok(canonical)
    }

    fn calculate_network_fee(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let raw = expect_base64_param_with_decode_message(
            params,
            0,
            "calculatenetworkfee",
            "Invalid transaction payload",
        )?;
        let transaction = Transaction::from_bytes(&raw).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params().with_data(format!("Invalid transaction: {err}")),
            )
        })?;
        let system = server.system();
        let store = system.store_cache();
        let wallet = server.wallet();
        let wallet_ref = wallet.as_deref();
        let fee = WalletHelper::calculate_network_fee_with_wallet(
            &transaction,
            store.data_cache(),
            system.settings(),
            wallet_ref,
            server.settings().max_gas_invoke,
        )
        .map_err(invalid_params)?;
        Ok(json!({ "networkfee": fee.to_string() }))
    }

    fn send_from(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let asset = parse_uint160(params, 0, "sendfrom")?;
        let from_hash =
            Self::parse_script_hash(server, &expect_string_param(params, 1, "sendfrom")?)?;
        let to_hash =
            Self::parse_script_hash(server, &expect_string_param(params, 2, "sendfrom")?)?;
        let amount_text = expect_string_param(params, 3, "sendfrom")?;
        let signers = Self::parse_optional_signers(server, params, 4)?;
        Self::process_transfer(
            server,
            asset,
            Some(from_hash),
            to_hash,
            amount_text,
            signers.as_deref(),
        )
        .map_err(Self::send_from_transfer_error)
    }

    fn send_to_address(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let asset = parse_uint160(params, 0, "sendtoaddress")?;
        let to_hash =
            Self::parse_script_hash(server, &expect_string_param(params, 1, "sendtoaddress")?)?;
        let amount_text = expect_string_param(params, 2, "sendtoaddress")?;
        let signers = Self::parse_optional_signers(server, params, 3)?;
        Self::process_transfer(
            server,
            asset,
            None,
            to_hash,
            amount_text,
            signers.as_deref(),
        )
        .map_err(Self::invalid_operation_transfer_error)
    }

    fn send_many(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        if params.is_empty() {
            return Err(invalid_params("sendmany requires at least one argument"));
        }
        let mut from: Option<UInt160> = None;
        let mut index = 0;
        if params[0].is_string() {
            from = Some(Self::parse_script_hash(
                server,
                &expect_string_param(params, 0, "sendmany")?,
            )?);
            index = 1;
        }

        let outputs_value = params.get(index).cloned().unwrap_or(Value::Null);
        let outputs_array = outputs_value
            .as_array()
            .ok_or_else(|| invalid_params(format!("Invalid 'to' parameter: {outputs_value}")))?;
        if outputs_array.is_empty() {
            return Err(invalid_params("Argument 'to' can't be empty."));
        }

        let signers = Self::parse_optional_signers(server, params, index + 1)?;

        let store = server.system().store_cache();
        let descriptor_cache = |asset: &UInt160| {
            AssetDescriptor::new(store.data_cache(), server.system().settings(), *asset)
        };

        let transfers = outputs_array
            .iter()
            .enumerate()
            .map(|(i, entry)| Self::parse_send_many_output(server, &descriptor_cache, i, entry))
            .collect::<Result<Vec<_>, _>>()?;

        Self::build_and_relay(server, &wallet, &transfers, from, signers.as_deref())
            .map_err(Self::invalid_operation_transfer_error)
    }

    fn cancel_transaction(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let txid = parse_uint256(params, 0, "canceltransaction")?;
        let signers_value = params
            .get(1)
            .ok_or_else(|| invalid_params("canceltransaction requires signers"))?;
        let signers_array = signers_value
            .as_array()
            .ok_or_else(|| invalid_params("canceltransaction signers must be an array"))?;
        if signers_array.is_empty() {
            return Err(RpcException::from(
                RpcError::bad_request().with_data("No signer."),
            ));
        }

        let mut signers = Vec::with_capacity(signers_array.len());
        for entry in signers_array {
            let address = entry
                .as_str()
                .ok_or_else(|| invalid_params("canceltransaction signers must be strings"))?;
            let hash = Self::parse_script_hash(server, address)?;
            signers.push(Signer::new(hash, WitnessScope::NONE));
        }

        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let snapshot = store.data_cache();
        if ledger
            .get_transaction_state(snapshot, &txid)
            .map_err(|err| internal_error(err.to_string()))?
            .is_some()
        {
            return Err(RpcException::from(
                RpcError::already_exists()
                    .with_data("This tx is already confirmed, can't be cancelled."),
            ));
        }

        let conflict_attr = TransactionAttribute::Conflicts(Conflicts::new(txid));
        let script = vec![OpCode::RET.byte()];
        let snapshot_arc = Arc::new(snapshot.clone());
        let mut tx = Helper::make_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            &script,
            Some(signers[0].account),
            Some(&signers),
            Some(std::slice::from_ref(&conflict_attr)),
            server.system().settings(),
            None,
            server.settings().max_gas_invoke,
        )
        .map_err(Self::wallet_failure)?;

        if let Some(conflict_tx) = server.system().mempool().lock().try_get(&txid) {
            let bumped = tx
                .network_fee()
                .max(conflict_tx.network_fee())
                .saturating_add(1);
            tx.set_network_fee(bumped);
        } else if let Some(extra_fee) = params.get(2).and_then(Value::as_str) {
            let decimals = GasToken::new().decimals();
            let (ok, fee) = BigDecimal::try_parse(extra_fee, decimals);
            let fee_amount = fee
                .value()
                .to_i64()
                .ok_or_else(|| invalid_params("Incorrect amount format."))?;
            if !ok || fee.sign() <= 0 {
                return Err(invalid_params("Incorrect amount format."));
            }
            tx.set_network_fee(tx.network_fee().saturating_add(fee_amount));
        }

        Self::sign_and_relay(server, &wallet, tx, snapshot_arc)
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

    fn parse_send_many_output(
        server: &RpcServer,
        descriptor_cache: &impl Fn(&UInt160) -> Result<AssetDescriptor, String>,
        index: usize,
        entry: &Value,
    ) -> Result<TransferOutput, RpcException> {
        let obj = entry
            .as_object()
            .ok_or_else(|| invalid_params(format!("Invalid 'to' parameter at {index}.")))?;
        let asset_str = obj
            .get("asset")
            .and_then(|value| value.as_str())
            .ok_or_else(|| invalid_params(format!("no 'asset' parameter at 'to[{index}]'.")))?;
        let asset = UInt160::from_str(asset_str)
            .map_err(|err| invalid_params(format!("invalid asset {asset_str}: {err}")))?;
        let descriptor = descriptor_cache(&asset).map_err(invalid_params)?;
        let value_str = obj
            .get("value")
            .and_then(|value| value.as_str())
            .ok_or_else(|| invalid_params(format!("no 'value' parameter at 'to[{index}]'.")))?;
        let (ok, value) = BigDecimal::try_parse(value_str, descriptor.decimals);
        if !ok {
            return Err(invalid_params(format!(
                "Invalid 'to' parameter at {index}."
            )));
        }
        if value.sign() <= 0 {
            return Err(invalid_params(format!(
                "Amount of '{asset}' can't be negative."
            )));
        }
        let address_str = obj
            .get("address")
            .and_then(|value| value.as_str())
            .ok_or_else(|| invalid_params(format!("no 'address' parameter at 'to[{index}]'.")))?;
        let to_hash = Self::parse_script_hash(server, address_str)?;
        Ok(TransferOutput {
            asset_id: asset,
            value,
            script_hash: to_hash,
            data: None,
        })
    }

    fn send_from_transfer_error(err: RpcException) -> RpcException {
        Self::map_insufficient_funds(err, |_| {
            RpcException::from(
                RpcError::invalid_request().with_data("Can not process this request."),
            )
        })
    }

    fn invalid_operation_transfer_error(err: RpcException) -> RpcException {
        Self::map_insufficient_funds(err, |rpc_error| {
            RpcException::new(INVALID_OPERATION_HRESULT, rpc_error.error_message())
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

    fn await_wallet_future<T: Send + 'static>(
        future: Pin<Box<dyn Future<Output = WalletResult<T>> + Send>>,
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

    fn account_to_json(account: &Arc<dyn WalletAccount>) -> Value {
        let has_key = account.has_key();
        let mut map = Map::new();
        map.insert("address".to_string(), Value::String(account.address()));
        map.insert("haskey".to_string(), Value::Bool(has_key));
        map.insert(
            "label".to_string(),
            account
                .label()
                .map_or(Value::Null, |label| Value::String(label.to_string())),
        );
        map.insert("watchonly".to_string(), Value::Bool(!has_key));
        Value::Object(map)
    }

    fn calculate_nep17_balance(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        asset: &UInt160,
    ) -> Result<BigDecimal, RpcException> {
        let accounts: Vec<UInt160> = wallet
            .get_accounts()
            .into_iter()
            .filter(|account| account.has_key())
            .map(|account| account.script_hash())
            .collect();
        if accounts.is_empty() {
            return Ok(Self::zero_balance());
        }

        let script = Self::build_balance_script(asset, &accounts)?;
        let store = server.system().store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            server.system().settings().clone(),
            server.settings().max_gas_invoke,
            None,
            neo_native_contracts::native::composition::default_native_services(),
        )
        .map_err(|err| internal_error(err.to_string()))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, Some(*asset))
            .map_err(|err| internal_error(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| internal_error(err.to_string()))?;
        if engine.state() == VMState::FAULT {
            return Ok(Self::zero_balance());
        }
        let decimals_value = engine
            .peek(0)
            .map_err(internal_error)?
            .as_int()
            .map_err(|err| internal_error(err.to_string()))?;
        let decimals = decimals_value
            .to_u8()
            .ok_or_else(|| invalid_params("invalid decimals value"))?;
        let amount_value = engine
            .peek(1)
            .map_err(internal_error)?
            .as_int()
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(BigDecimal::new(amount_value, decimals))
    }

    fn build_balance_script(
        asset: &UInt160,
        accounts: &[UInt160],
    ) -> Result<Vec<u8>, RpcException> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        for account in accounts {
            let account_bytes = account.to_bytes();
            Self::emit_contract_call(
                &mut builder,
                asset,
                "balanceOf",
                &[account_bytes.as_slice()],
                CallFlags::READ_ONLY,
            )?;
            builder.emit_opcode(OpCode::ADD);
        }
        Self::emit_contract_call(&mut builder, asset, "decimals", &[], CallFlags::READ_ONLY)?;
        Ok(builder.to_array())
    }

    fn emit_contract_call(
        builder: &mut ScriptBuilder,
        contract: &UInt160,
        method: &str,
        args: &[&[u8]],
        flags: CallFlags,
    ) -> Result<(), RpcException> {
        if args.is_empty() {
            builder.emit_opcode(OpCode::NEWARRAY0);
        } else {
            for arg in args.iter().rev() {
                builder.emit_push(arg);
            }
            builder.emit_push_int(args.len() as i64);
            builder.emit_opcode(OpCode::PACK);
        }

        builder.emit_push_int(i64::from(flags.bits()));
        builder.emit_push(method.as_bytes());
        let hash_bytes = contract.to_bytes();
        builder.emit_push(&hash_bytes);
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(())
    }

    fn zero_balance() -> BigDecimal {
        BigDecimal::new(BigInt::zero(), 0)
    }

    fn require_wallet(server: &RpcServer) -> Result<Arc<dyn CoreWallet>, RpcException> {
        server
            .wallet()
            .ok_or_else(|| RpcException::from(RpcError::no_opened_wallet()))
    }

    fn process_transfer(
        server: &RpcServer,
        asset: UInt160,
        from: Option<UInt160>,
        to: UInt160,
        amount: String,
        signers: Option<&[Signer]>,
    ) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let descriptor =
            AssetDescriptor::new(store.data_cache(), server.system().settings(), asset)
                .map_err(invalid_params)?;
        let (ok, value) = BigDecimal::try_parse(&amount, descriptor.decimals);
        if !ok || value.sign() <= 0 {
            return Err(invalid_params("Amount can't be negative."));
        }

        let transfer = TransferOutput {
            asset_id: asset,
            value,
            script_hash: to,
            data: None,
        };

        let tx_json = Self::build_and_relay(server, &wallet, &[transfer], from, signers)?;
        Ok(tx_json)
    }

    fn build_and_relay(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        outputs: &[TransferOutput],
        from: Option<UInt160>,
        signers: Option<&[Signer]>,
    ) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let snapshot_arc = Arc::new(store.data_cache().clone());
        let tx = Helper::make_transfer_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            outputs,
            from,
            signers,
            server.system().settings(),
            None,
            server.settings().max_gas_invoke,
        )
        .map_err(Self::wallet_failure)?;

        Self::sign_and_relay(server, wallet, tx, snapshot_arc)
    }

    fn sign_and_relay(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        tx: Transaction,
        snapshot_arc: Arc<DataCache>,
    ) -> Result<Value, RpcException> {
        Self::sign_and_relay_with_observers(server, wallet, tx, snapshot_arc, || {}, || {})
    }

    fn sign_and_relay_with_observers(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        tx: Transaction,
        snapshot_arc: Arc<DataCache>,
        after_authorize: impl FnOnce(),
        before_relay: impl FnOnce(),
    ) -> Result<Value, RpcException> {
        let prepared = Self::prepare_transaction(server, wallet, tx, snapshot_arc.as_ref())?;
        let tx = match Self::authorize_transaction(server, wallet, prepared, snapshot_arc.clone())?
        {
            WalletAuthorization::Signed(tx) => tx,
            WalletAuthorization::Pending(context) => return Ok(context),
        };
        after_authorize();
        let current_snapshot = server.system().store_cache();
        Self::validate_transaction_before_signing(server, &tx, &current_snapshot.data_cache())?;
        before_relay();

        match rpc_relay::with_relay_responder(server, |sender| {
            server
                .system()
                .tx_router_actor()
                .try_enqueue_preverify_from(tx.clone(), true, Some(sender))
                .map_err(|err| internal_error(err.to_string()))
        }) {
            Ok(relay_result) => {
                rpc_relay::map_relay_result(relay_result)?;
                Ok(tx.to_json(server.system().settings()))
            }
            Err(err) => {
                // Preverify failure: surface unsigned context
                let mut context = ContractParametersContext::new_with_type(
                    snapshot_arc,
                    tx.clone(),
                    server.system().settings().network,
                    Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
                );
                for signer in tx.signers() {
                    if let Some(account) = wallet.get_account(&signer.account)
                        && let Some(contract) = account.contract()
                    {
                        context.add_contract(contract.clone());
                    }
                }
                let mut json = context.to_json();
                // Attach reason
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("preverifyFail".to_string(), Value::String(err.to_string()));
                }
                Ok(json)
            }
        }
    }

    fn prepare_transaction(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        mut tx: Transaction,
        snapshot: &DataCache,
    ) -> Result<PreparedTransaction, RpcException> {
        let projected_size = Self::projected_signed_size(&tx, wallet);
        if projected_size > 1024 {
            let fee_per_byte = PolicyContract::new()
                .get_fee_per_byte_snapshot(snapshot)
                .unwrap_or(i64::from(PolicyContract::DEFAULT_FEE_PER_BYTE));
            let projected_size = i64::try_from(projected_size)
                .map_err(|_| internal_error("Transaction size overflow".to_string()))?;
            let minimum_fee = projected_size
                .checked_mul(fee_per_byte)
                .and_then(|fee| fee.checked_add(100_000))
                .ok_or_else(|| internal_error("Network fee overflow".to_string()))?;
            if tx.network_fee() < minimum_fee {
                tx.set_network_fee(minimum_fee);
            }
        }

        Self::validate_transaction_before_signing(server, &tx, snapshot)?;
        let unsigned_data = tx
            .try_get_hash_data()
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(PreparedTransaction {
            transaction: tx,
            unsigned_data,
        })
    }

    fn validate_transaction_before_signing(
        server: &RpcServer,
        tx: &Transaction,
        snapshot: &DataCache,
    ) -> Result<(), RpcException> {
        if tx.version() != 0 {
            return Err(invalid_params("Invalid transaction version"));
        }
        if tx.signers().is_empty() {
            return Err(invalid_params("Transaction must have at least one signer"));
        }
        if tx.script().is_empty() {
            return Err(invalid_params("Transaction script cannot be empty"));
        }
        neo_vm::validate_strict_script(tx.script())
            .map_err(|err| invalid_params(format!("Invalid transaction script: {err}")))?;
        if tx.system_fee() < 0 {
            return Err(invalid_params("Invalid system fee"));
        }
        if tx.network_fee() < 0 {
            return Err(invalid_params("Invalid network fee"));
        }
        let required_fee = tx
            .system_fee()
            .checked_add(tx.network_fee())
            .ok_or_else(|| invalid_params("Combined transaction fee overflow"))?;
        if tx.network_fee() > server.settings().max_fee {
            return Err(RpcException::from(RpcError::wallet_fee_limit()));
        }

        let current_height = LedgerContract::new()
            .current_index(snapshot)
            .map_err(|err| internal_error(err.to_string()))?;
        let max_increment = PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(snapshot, server.system().settings())
            .unwrap_or(server.system().settings().max_valid_until_block_increment);
        if tx.valid_until_block() <= current_height
            || tx.valid_until_block() > current_height.saturating_add(max_increment)
        {
            return Err(invalid_params("Invalid valid-until-block"));
        }

        let sender = tx
            .sender()
            .ok_or_else(|| invalid_params("Transaction must have a sender"))?;
        if GasToken::new().balance_of_snapshot(snapshot, &sender) < BigInt::from(required_fee) {
            return Err(RpcException::from(RpcError::insufficient_funds_wallet()));
        }

        let mut validation_tx = tx.clone();
        validation_tx.set_witnesses(vec![Witness::new(); tx.signers().len()]);
        let bytes = validation_tx
            .try_to_bytes()
            .map_err(|err| invalid_params(format!("Invalid transaction fields: {err}")))?;
        Transaction::from_bytes(&bytes)
            .map_err(|err| invalid_params(format!("Invalid signer or witness scope: {err}")))?;
        Ok(())
    }

    fn projected_signed_size(tx: &Transaction, wallet: &Arc<dyn CoreWallet>) -> usize {
        let witnesses = tx
            .signers()
            .iter()
            .map(|signer| {
                let Some(account) = wallet.get_account(&signer.account) else {
                    return Witness::new();
                };
                let Some(contract) = account.contract().cloned() else {
                    return Witness::new();
                };
                let signature_count = contract
                    .parameter_list
                    .iter()
                    .filter(|parameter| {
                        **parameter == neo_primitives::ContractParameterType::Signature
                    })
                    .count();
                Witness::new_with_scripts(
                    vec![0; signature_count * SIGNATURE_INVOCATION_SCRIPT_LENGTH],
                    contract.script,
                )
            })
            .collect();
        let mut projected = tx.clone();
        projected.set_witnesses(witnesses);
        projected.size()
    }

    fn authorize_transaction(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        prepared: PreparedTransaction,
        snapshot_arc: Arc<DataCache>,
    ) -> Result<WalletAuthorization, RpcException> {
        let mut sign_data: Option<Vec<u8>> = None;
        let tx = prepared.transaction();
        let mut context = ContractParametersContext::new_with_type(
            snapshot_arc,
            tx.clone(),
            server.system().settings().network,
            Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
        );
        let signer_accounts: Vec<UInt160> =
            tx.signers().iter().map(|signer| signer.account).collect();
        for signer_account in signer_accounts {
            let Some(account) = wallet.get_account(&signer_account) else {
                continue;
            };
            let Some(contract) = account.contract().cloned() else {
                continue;
            };
            context.add_contract(contract.clone());
            if account.is_locked() || !account.has_key() {
                continue;
            }

            let canonical_data = if let Some(data) = sign_data.as_ref() {
                data.clone()
            } else {
                let data = neo_protocol::get_sign_data_vec(tx, server.system().settings().network)
                    .map_err(|err| internal_error(err.to_string()))?;
                sign_data = Some(data.clone());
                data
            };
            let signature = if let Some(runtime) = server.signer_runtime() {
                Self::sign_transaction_with_service(
                    server,
                    runtime,
                    tx,
                    signer_account,
                    canonical_data,
                )?
            } else {
                #[cfg(not(test))]
                {
                    return Err(internal_error(
                        "RPC wallet signing requires the unified signer service".to_string(),
                    ));
                }
                #[cfg(test)]
                {
                    let wallet_clone = Arc::clone(wallet);
                    Self::await_wallet_future(Box::pin(async move {
                        wallet_clone.sign(&canonical_data, &signer_account).await
                    }))?
                }
            };
            if signature.len() != 64 {
                return Err(internal_error(
                    "Invalid signature length from signer service".to_string(),
                ));
            }
            let pub_key_bytes = signature_contract_pubkey(&contract.script)?;
            let pub_key = ECPoint::new(ECCurve::Secp256r1, pub_key_bytes)
                .map_err(|err| internal_error(err.to_string()))?;
            let _ = context.add_signature(contract, pub_key, signature);
        }

        if !context.completed() {
            return Ok(WalletAuthorization::Pending(context.to_json()));
        }
        let witnesses = context.witnesses().ok_or_else(|| {
            internal_error("Completed signature context has no witnesses".to_string())
        })?;
        Ok(WalletAuthorization::Signed(prepared.complete(witnesses)?))
    }

    fn sign_transaction_with_service(
        server: &RpcServer,
        runtime: &RpcSignerRuntime,
        tx: &Transaction,
        signer_account: UInt160,
        canonical_data: Vec<u8>,
    ) -> Result<Vec<u8>, RpcException> {
        let network = server.system().settings().network;
        let tx_hash = tx.hash().to_string();
        let account = signer_account.to_string();
        let request_id = format!("rpc:{network}:{tx_hash}:{account}");
        let fee_units = tx.system_fee().saturating_add(tx.network_fee()).max(0) as u64;
        let key_ref = runtime
            .resolve_key(&account, &SignPurpose::Transaction)
            .map_err(|error| internal_error(error.to_string()))?;
        let intent = FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network,
            purpose: SignPurpose::Transaction,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: "neo-n3-sign-data-v1".to_string(),
                bytes: canonical_data,
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-transaction".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::from([
                    ("transaction_hash".to_string(), tx_hash),
                    ("nonce".to_string(), tx.nonce().to_string()),
                    (
                        "valid_until_block".to_string(),
                        tx.valid_until_block().to_string(),
                    ),
                    ("signer_account".to_string(), account.clone()),
                ]),
            },
            policy_version: runtime.policy_version.clone(),
            fee_units,
            quota_units: 1,
            account: Some(account),
            targets: BTreeSet::new(),
        };
        let runtime = runtime.clone();
        Self::await_signer_future(Box::pin(async move {
            runtime.resume_or_start(&request_id, intent).await
        }))
        .map(|response| response.signature)
    }

    fn await_signer_future(
        future: Pin<Box<dyn Future<Output = neo_signing::SignResult<SignResponse>> + Send>>,
    ) -> Result<SignResponse, RpcException> {
        let result = if let Ok(handle) = Handle::try_current() {
            match handle.runtime_flavor() {
                RuntimeFlavor::CurrentThread => std::thread::spawn(move || {
                    RuntimeBuilder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            neo_signing::SignError::new(
                                neo_signing::SignErrorCode::ProviderFailure,
                                error.to_string(),
                            )
                        })?
                        .block_on(future)
                })
                .join()
                .map_err(|_| internal_error("signer runtime thread panicked".to_string()))?,
                RuntimeFlavor::MultiThread => {
                    tokio::task::block_in_place(move || handle.block_on(future))
                }
                _ => tokio::task::block_in_place(move || handle.block_on(future)),
            }
        } else {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| internal_error(error.to_string()))?
                .block_on(future)
        };
        result.map_err(|error| {
            internal_error(format!(
                "signer service rejected transaction authorization ({:?}): {}",
                error.code, error.message
            ))
        })
    }
}

fn signature_contract_pubkey(script: &[u8]) -> Result<Vec<u8>, RpcException> {
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

#[cfg(test)]
mod tests;
