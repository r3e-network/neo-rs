//! RPC wallet endpoints mirroring RpcServer.Wallet.cs.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::big_decimal::BigDecimal;
use neo_core::cryptography::{ECCurve, ECPoint};
use neo_core::ledger::{RelayResult, VerifyResult};
use neo_core::neo_system::TransactionRouterMessage;
use neo_core::network::p2p::payloads::conflicts::Conflicts;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::persistence::DataCache;
use neo_core::prelude::Serializable;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::contract_parameters_context::ContractParametersContext;
use neo_core::smart_contract::helper::Helper as ContractHelper;
use neo_core::smart_contract::native::{
    GasToken, LedgerContract, NativeContract, NeoToken, PolicyContract,
};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::{
    AssetDescriptor, Helper, KeyPair, Nep6Wallet, TransferOutput, Wallet as CoreWallet,
    WalletAccount, WalletError, WalletResult,
};
use neo_core::{UInt160, UInt256, WitnessScope};
use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::vm_state::VMState;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use serde_json::{json, Map, Value};
use std::future::Future;
use std::io::ErrorKind;
use std::path::Path;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tokio::runtime::{Builder as RuntimeBuilder, Handle};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_server::{RpcHandler, RpcServer};

pub struct RpcServerWallet;

const INVALID_OPERATION_HRESULT: i32 = -2146233079;

impl RpcServerWallet {
    /// Registers all wallet RPC handlers.
    ///
    /// # Security
    /// Wallet methods are marked as protected metadata. Authentication is enforced
    /// only when RPC basic auth is configured, matching C# behavior.
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            // Wallet methods are marked as protected metadata.
            Self::protected_handler("closewallet", Self::close_wallet),
            Self::protected_handler("dumpprivkey", Self::dump_priv_key),
            Self::protected_handler("getnewaddress", Self::get_new_address),
            Self::protected_handler("getwalletbalance", Self::get_wallet_balance),
            Self::protected_handler("getwalletunclaimedgas", Self::get_wallet_unclaimed_gas),
            Self::protected_handler("importprivkey", Self::import_priv_key),
            Self::protected_handler("listaddress", Self::list_address),
            Self::protected_handler("openwallet", Self::open_wallet),
            Self::protected_handler("calculatenetworkfee", Self::calculate_network_fee),
            Self::protected_handler("sendfrom", Self::send_from),
            Self::protected_handler("sendtoaddress", Self::send_to_address),
            Self::protected_handler("sendmany", Self::send_many),
            Self::protected_handler("canceltransaction", Self::cancel_transaction),
        ]
    }

    /// Creates a protected handler for wallet operations.
    fn protected_handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new_protected(name), Arc::new(func))
    }

    fn close_wallet(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        server.set_wallet(None);
        Ok(Value::Bool(true))
    }

    fn dump_priv_key(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let address = Self::expect_string_param(params, 0, "dumpprivkey")?;
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
        let asset = Self::parse_uint160(params, 0, "getwalletbalance")?;
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
        let privkey = Self::expect_string_param(params, 0, "importprivkey")?;
        KeyPair::from_wif(&privkey)
            .map_err(|err| Self::invalid_params(format!("invalid WIF: {err}")))?;
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
        let path = Self::expect_string_param(params, 0, "openwallet")?;
        let password = Self::expect_string_param(params, 1, "openwallet")?;
        if !Path::new(&path).exists() {
            return Err(RpcException::from(RpcError::wallet_not_found()));
        }
        let system = server.system();
        let settings = Arc::new(system.settings().clone());
        let wallet = Nep6Wallet::from_file(&path, &password, settings);
        let wallet = match wallet {
            Ok(wallet) => wallet,
            Err(WalletError::InvalidPassword) => {
                return Err(RpcException::from(
                    RpcError::wallet_not_supported().with_data("Invalid password."),
                ))
            }
            Err(WalletError::WalletFileNotFound(_)) => {
                return Err(RpcException::from(RpcError::wallet_not_found()))
            }
            Err(WalletError::Io(ref err)) if err.kind() == ErrorKind::NotFound => {
                return Err(RpcException::from(RpcError::wallet_not_found()))
            }
            Err(err) => {
                return Err(RpcException::from(
                    RpcError::wallet_not_supported().with_data(err.to_string()),
                ))
            }
        };
        let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
        server.set_wallet(Some(wallet_arc));
        Ok(Value::Bool(true))
    }

    fn calculate_network_fee(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let payload = Self::expect_string_param(params, 0, "calculatenetworkfee")?;
        let raw = BASE64_STANDARD.decode(payload.trim()).map_err(|_| {
            RpcException::from(RpcError::invalid_params().with_data("Invalid transaction payload"))
        })?;
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
        .map_err(Self::invalid_params)?;
        Ok(json!({ "networkfee": fee.to_string() }))
    }

    fn send_from(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let asset = Self::parse_uint160(params, 0, "sendfrom")?;
        let from_hash =
            Self::parse_script_hash(server, &Self::expect_string_param(params, 1, "sendfrom")?)?;
        let to_hash =
            Self::parse_script_hash(server, &Self::expect_string_param(params, 2, "sendfrom")?)?;
        let amount_text = Self::expect_string_param(params, 3, "sendfrom")?;
        let signers = if params.len() > 4 {
            Some(Self::parse_signers(server, &params[4])?)
        } else {
            None
        };
        match Self::process_transfer(
            server,
            asset,
            Some(from_hash),
            to_hash,
            amount_text,
            signers.as_deref(),
        ) {
            Ok(value) => Ok(value),
            Err(err) => {
                let rpc_error: RpcError = err.into();
                if rpc_error.code() == RpcError::insufficient_funds_wallet().code() {
                    return Err(RpcException::from(
                        RpcError::invalid_request().with_data("Can not process this request."),
                    ));
                }
                Err(RpcException::from(rpc_error))
            }
        }
    }

    fn send_to_address(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let asset = Self::parse_uint160(params, 0, "sendtoaddress")?;
        let to_hash = Self::parse_script_hash(
            server,
            &Self::expect_string_param(params, 1, "sendtoaddress")?,
        )?;
        let amount_text = Self::expect_string_param(params, 2, "sendtoaddress")?;
        let signers = if params.len() > 3 {
            Some(Self::parse_signers(server, &params[3])?)
        } else {
            None
        };
        match Self::process_transfer(
            server,
            asset,
            None,
            to_hash,
            amount_text,
            signers.as_deref(),
        ) {
            Ok(value) => Ok(value),
            Err(err) => {
                let rpc_error: RpcError = err.into();
                if rpc_error.code() == RpcError::insufficient_funds_wallet().code() {
                    return Err(RpcException::new(
                        INVALID_OPERATION_HRESULT,
                        rpc_error.error_message(),
                    ));
                }
                Err(RpcException::from(rpc_error))
            }
        }
    }

    fn send_many(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        if params.is_empty() {
            return Err(Self::invalid_params(
                "sendmany requires at least one argument",
            ));
        }
        let mut from: Option<UInt160> = None;
        let mut index = 0;
        if params[0].is_string() {
            from = Some(Self::parse_script_hash(
                server,
                &Self::expect_string_param(params, 0, "sendmany")?,
            )?);
            index = 1;
        }

        let outputs_value = params.get(index).cloned().unwrap_or(Value::Null);
        let outputs_array = outputs_value.as_array().ok_or_else(|| {
            Self::invalid_params(format!("Invalid 'to' parameter: {outputs_value}"))
        })?;
        if outputs_array.is_empty() {
            return Err(Self::invalid_params("Argument 'to' can't be empty."));
        }

        let signers = if params.len() > index + 1 {
            Some(Self::parse_signers(server, &params[index + 1])?)
        } else {
            None
        };

        let store = server.system().store_cache();
        let descriptor_cache = |asset: &UInt160| {
            AssetDescriptor::new(store.data_cache(), server.system().settings(), *asset)
        };

        let mut transfers = Vec::new();
        for (i, entry) in outputs_array.iter().enumerate() {
            let obj = entry
                .as_object()
                .ok_or_else(|| Self::invalid_params(format!("Invalid 'to' parameter at {i}.")))?;
            let asset_str = obj.get("asset").and_then(|v| v.as_str()).ok_or_else(|| {
                Self::invalid_params(format!("no 'asset' parameter at 'to[{i}]'."))
            })?;
            let asset = UInt160::from_str(asset_str)
                .map_err(|e| Self::invalid_params(format!("invalid asset {asset_str}: {e}")))?;
            let descriptor =
                descriptor_cache(&asset).map_err(Self::invalid_params)?;
            let value_str = obj.get("value").and_then(|v| v.as_str()).ok_or_else(|| {
                Self::invalid_params(format!("no 'value' parameter at 'to[{i}]'."))
            })?;
            let (ok, value) = BigDecimal::try_parse(value_str, descriptor.decimals);
            if !ok {
                return Err(Self::invalid_params(format!(
                    "Invalid 'to' parameter at {i}."
                )));
            }
            if value.sign() <= 0 {
                return Err(Self::invalid_params(format!(
                    "Amount of '{asset}' can't be negative."
                )));
            }
            let address_str = obj.get("address").and_then(|v| v.as_str()).ok_or_else(|| {
                Self::invalid_params(format!("no 'address' parameter at 'to[{i}]'."))
            })?;
            let to_hash = Self::parse_script_hash(server, address_str)?;
            transfers.push(TransferOutput {
                asset_id: asset,
                value,
                script_hash: to_hash,
                data: None,
            });
        }

        let tx_json =
            match Self::build_and_relay(server, &wallet, &transfers, from, signers.as_deref()) {
                Ok(value) => value,
                Err(err) => {
                    let rpc_error: RpcError = err.into();
                    if rpc_error.code() == RpcError::insufficient_funds_wallet().code() {
                        return Err(RpcException::new(
                            INVALID_OPERATION_HRESULT,
                            rpc_error.error_message(),
                        ));
                    }
                    return Err(RpcException::from(rpc_error));
                }
            };
        Ok(tx_json)
    }

    fn cancel_transaction(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let txid = Self::parse_uint256(params, 0, "canceltransaction")?;
        let signers_value = params
            .get(1)
            .ok_or_else(|| Self::invalid_params("canceltransaction requires signers"))?;
        let signers_array = signers_value
            .as_array()
            .ok_or_else(|| Self::invalid_params("canceltransaction signers must be an array"))?;
        if signers_array.is_empty() {
            return Err(RpcException::from(
                RpcError::bad_request().with_data("No signer."),
            ));
        }

        let mut signers = Vec::with_capacity(signers_array.len());
        for entry in signers_array {
            let address = entry
                .as_str()
                .ok_or_else(|| Self::invalid_params("canceltransaction signers must be strings"))?;
            let hash = Self::parse_script_hash(server, address)?;
            signers.push(Signer::new(hash, WitnessScope::NONE));
        }

        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let snapshot = store.data_cache();
        if ledger
            .get_transaction_state(snapshot, &txid)
            .map_err(|err| Self::internal_error(err.to_string()))?
            .is_some()
        {
            return Err(RpcException::from(
                RpcError::already_exists()
                    .with_data("This tx is already confirmed, can't be cancelled."),
            ));
        }

        let conflict_attr = TransactionAttribute::Conflicts(Conflicts::new(txid));
        let script = vec![OpCode::RET as u8];
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
                .ok_or_else(|| Self::invalid_params("Incorrect amount format."))?;
            if !ok || fee.sign() <= 0 {
                return Err(Self::invalid_params("Incorrect amount format."));
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
        WalletHelper::to_script_hash(value, version).map_err(Self::invalid_params)
    }

    fn parse_signers(server: &RpcServer, value: &Value) -> Result<Vec<Signer>, RpcException> {
        let array = value
            .as_array()
            .ok_or_else(|| Self::invalid_params("signers must be an array"))?;
        let mut signers = Vec::with_capacity(array.len());
        for entry in array {
            let addr = entry
                .as_str()
                .ok_or_else(|| Self::invalid_params("signer entries must be strings"))?;
            let hash = Self::parse_script_hash(server, addr)?;
            signers.push(Signer::new(hash, WitnessScope::CALLED_BY_ENTRY));
        }
        Ok(signers)
    }

    fn parse_uint160(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<UInt160, RpcException> {
        let text = Self::expect_string_param(params, index, method)?;
        UInt160::from_str(&text).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("invalid UInt160 '{text}': {err}")),
            )
        })
    }

    fn parse_uint256(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<UInt256, RpcException> {
        let text = Self::expect_string_param(params, index, method)?;
        UInt256::from_str(&text).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("invalid UInt256 '{text}': {err}")),
            )
        })
    }

    fn expect_string_param(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<String, RpcException> {
        params
            .get(index)
            .and_then(|value| value.as_str())
            .map(std::string::ToString::to_string)
            .ok_or_else(|| {
                RpcException::from(RpcError::invalid_params().with_data(format!(
                    "{} expects string parameter {}",
                    method,
                    index + 1
                )))
            })
    }

    fn invalid_params(message: impl Into<String>) -> RpcException {
        RpcException::from(RpcError::invalid_params().with_data(message.into()))
    }

    fn internal_error(message: impl Into<String>) -> RpcException {
        RpcException::from(RpcError::internal_server_error().with_data(message.into()))
    }

    fn await_wallet_future<T>(
        future: Pin<Box<dyn Future<Output = WalletResult<T>> + Send>>,
    ) -> Result<T, RpcException> {
        let result = if let Ok(handle) = Handle::try_current() {
            handle.block_on(future)
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
        )
        .map_err(|err| Self::internal_error(err.to_string()))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, Some(*asset))
            .map_err(|err| Self::internal_error(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| Self::internal_error(err.to_string()))?;
        if engine.state() == VMState::FAULT {
            return Ok(Self::zero_balance());
        }
        let decimals_value = engine
            .peek(0)
            .map_err(Self::internal_error)?
            .as_int()
            .map_err(|err| Self::internal_error(err.to_string()))?;
        let decimals = decimals_value
            .to_u8()
            .ok_or_else(|| Self::invalid_params("invalid decimals value"))?;
        let amount_value = engine
            .peek(1)
            .map_err(Self::internal_error)?
            .as_int()
            .map_err(|err| Self::internal_error(err.to_string()))?;
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
            .map_err(|err| Self::internal_error(err.to_string()))?;
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
                .map_err(Self::invalid_params)?;
        let (ok, value) = BigDecimal::try_parse(&amount, descriptor.decimals);
        if !ok || value.sign() <= 0 {
            return Err(Self::invalid_params("Amount can't be negative."));
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
        mut tx: Transaction,
        snapshot_arc: Arc<DataCache>,
    ) -> Result<Value, RpcException> {
        let mut sign_data: Option<Vec<u8>> = None;

        // Build contract parameter context and add signatures from available keys
        let mut context = ContractParametersContext::new_with_type(
            snapshot_arc.clone(),
            tx.clone(),
            server.system().settings().network,
            Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
        );
        let signer_accounts: Vec<UInt160> =
            tx.signers().iter().map(|signer| signer.account).collect();
        for signer_account in signer_accounts {
            if let Some(account) = wallet.get_account(&signer_account) {
                let mut contract_opt = account.contract().cloned();
                let key_opt = account.get_key();
                if contract_opt.is_none() {
                    if let Some(ref key) = key_opt {
                        let pub_point = key
                            .get_public_key_point()
                            .ok()
                            .and_then(|p| ECPoint::from_bytes(&p.to_bytes()).ok());
                        if let Some(point) = pub_point {
                            contract_opt = Some(Contract::create_signature_contract(point));
                        }
                    }
                }

                if let Some(contract) = contract_opt {
                    context.add_contract(contract.clone());
                    if let Some(key) = key_opt {
                        if account.has_key() && !account.is_locked() {
                            let signature =
                                Helper::sign(&tx, &key, server.system().settings().network)
                                    .map_err(Self::internal_error)?;
                            // Neo N3 uses secp256r1 (NIST P-256) curve
                            let pub_key =
                                ECPoint::new(ECCurve::Secp256r1, key.compressed_public_key())
                                    .map_err(|e| Self::internal_error(e.to_string()))?;
                            let _ = context.add_signature(contract.clone(), pub_key, signature);
                        }
                    } else if account.has_key() && !account.is_locked() {
                        let sign_data = if let Some(data) = sign_data.as_ref() { data.clone() } else {
                            let data = neo_core::network::p2p::helper::get_sign_data_vec(
                                &tx,
                                server.system().settings().network,
                            )
                            .map_err(|err| Self::internal_error(err.to_string()))?;
                            sign_data = Some(data.clone());
                            data
                        };
                        let wallet_clone = Arc::clone(wallet);
                        let signature = Self::await_wallet_future(Box::pin(async move {
                            wallet_clone.sign(&sign_data, &signer_account).await
                        }))?;
                        if signature.len() != 64 {
                            return Err(Self::internal_error(
                                "Invalid signature length from wallet".to_string(),
                            ));
                        }
                        let pub_key_bytes = signature_contract_pubkey(&contract.script)?;
                        let pub_key = ECPoint::new(ECCurve::Secp256r1, pub_key_bytes)
                            .map_err(|e| Self::internal_error(e.to_string()))?;
                        let _ = context.add_signature(contract.clone(), pub_key, signature);
                    }
                }
            }
        }

        if !context.completed() {
            return Ok(context.to_json());
        }

        if let Some(witnesses) = context.get_witnesses() {
            tx.set_witnesses(witnesses);
        }

        // Adjust network fee if necessary (parity with C# min fee calculation)
        if tx.size() > 1024 {
            let policy = PolicyContract::new();
            let fee_per_byte = policy
                .get_fee_per_byte_snapshot(snapshot_arc.as_ref())
                .unwrap_or(i64::from(PolicyContract::DEFAULT_FEE_PER_BYTE));
            let cal_fee = tx.size() as i64 * fee_per_byte + 100_000;
            if tx.network_fee() < cal_fee {
                tx.set_network_fee(cal_fee);
            }
        }
        if tx.network_fee() > server.settings().max_fee {
            return Err(RpcException::from(RpcError::wallet_fee_limit()));
        }

        match Self::with_relay_responder(server, |sender| {
            server
                .system()
                .tx_router_actor()
                .tell_from(
                    TransactionRouterMessage::Preverify {
                        transaction: tx.clone(),
                        relay: true,
                    },
                    Some(sender),
                )
                .map_err(|err| Self::internal_error(err.to_string()))
        }) {
            Ok(relay_result) => {
                Self::map_relay_result(relay_result)?;
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
                    if let Some(account) = wallet.get_account(&signer.account) {
                        if let Some(contract) = account.contract() {
                            context.add_contract(contract.clone());
                        }
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

    fn map_relay_result(result: RelayResult) -> Result<Value, RpcException> {
        match result.result {
            VerifyResult::Succeed => Ok(json!({ "hash": result.hash.to_string() })),
            VerifyResult::AlreadyExists => Err(RpcException::from(RpcError::already_exists())),
            VerifyResult::AlreadyInPool => Err(RpcException::from(RpcError::already_in_pool())),
            VerifyResult::OutOfMemory => Err(RpcException::from(RpcError::mempool_cap_reached())),
            VerifyResult::InvalidScript => Err(RpcException::from(RpcError::invalid_script())),
            VerifyResult::InvalidAttribute => {
                Err(RpcException::from(RpcError::invalid_attribute()))
            }
            VerifyResult::InvalidSignature => {
                Err(RpcException::from(RpcError::invalid_signature()))
            }
            VerifyResult::OverSize => Err(RpcException::from(RpcError::invalid_size())),
            VerifyResult::Expired => Err(RpcException::from(RpcError::expired_transaction())),
            VerifyResult::InsufficientFunds => {
                Err(RpcException::from(RpcError::insufficient_funds()))
            }
            VerifyResult::PolicyFail => Err(RpcException::from(RpcError::policy_failed())),
            VerifyResult::UnableToVerify => Err(RpcException::from(
                RpcError::verification_failed().with_data("UnableToVerify"),
            )),
            VerifyResult::Invalid => Err(RpcException::from(
                RpcError::verification_failed().with_data("Invalid"),
            )),
            VerifyResult::HasConflicts => Err(RpcException::from(
                RpcError::verification_failed().with_data("HasConflicts"),
            )),
            VerifyResult::Unknown => Err(RpcException::from(
                RpcError::verification_failed().with_data("Unknown"),
            )),
        }
    }

    fn with_relay_responder<F>(server: &RpcServer, send: F) -> Result<RelayResult, RpcException>
    where
        F: FnOnce(neo_core::akka::ActorRef) -> Result<(), RpcException>,
    {
        use async_trait::async_trait;
        use neo_core::akka::{Actor, ActorContext, ActorResult, Props};
        use neo_core::ledger::RelayResult;
        use parking_lot::Mutex;

        struct RelayResponder {
            tx: std::sync::Arc<Mutex<Option<std::sync::mpsc::Sender<RelayResult>>>>,
        }

        #[async_trait]
        impl Actor for RelayResponder {
            async fn pre_start(&mut self, _ctx: &mut ActorContext) -> ActorResult {
                Ok(())
            }
            async fn handle(
                &mut self,
                msg: Box<dyn std::any::Any + Send>,
                _ctx: &mut ActorContext,
            ) -> ActorResult {
                if let Ok(result) = msg.downcast::<RelayResult>() {
                    let mut tx_guard = self.tx.lock();
                    if let Some(sender) = tx_guard.take() {
                        let _ = sender.send(*result);
                    }
                }
                Ok(())
            }
        }

        let system = server.system();
        let actor_system = system.actor_system();
        let (tx, rx) = std::sync::mpsc::channel();
        let responder = RelayResponder {
            tx: std::sync::Arc::new(Mutex::new(Some(tx))),
        };
        let props = Props::new(move || RelayResponder {
            tx: std::sync::Arc::clone(&responder.tx),
        });
        let actor_ref = actor_system
            .actor_of(props, format!("relay_responder_{}", Uuid::new_v4()))
            .map_err(|err| Self::internal_error(err.to_string()))?;

        send(actor_ref)?;

        let result = rx
            .recv()
            .map_err(|err| Self::internal_error(err.to_string()))?;
        Ok(result)
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
mod tests {
    use super::*;
    use crate::server::rcp_server_settings::RpcServerConfig;
    use neo_core::neo_io::BinaryWriter;
    use neo_core::network::p2p::helper::get_sign_data_vec;
    use neo_core::network::p2p::payloads::conflicts::Conflicts;
    use neo_core::network::p2p::payloads::signer::Signer;
    use neo_core::network::p2p::payloads::transaction::Transaction;
    use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
    use neo_core::protocol_settings::ProtocolSettings;
    use neo_core::smart_contract::helper::Helper as ContractHelper;
    use neo_core::smart_contract::native::LedgerContract;
    use neo_core::smart_contract::{StorageItem, StorageKey};
    use neo_core::IVerifiable;
    use neo_core::NeoSystem;
    use neo_core::UInt256;
    use neo_core::Witness;
    use neo_crypto::Secp256r1Crypto;
    use neo_vm::vm_state::VMState;
    use num_bigint::BigInt;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::runtime::{Handle, Runtime};

    fn temp_wallet_path() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("timestamp")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("rpc_wallet_{nanos}.json"))
            .to_string_lossy()
            .to_string()
    }

    fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
        handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
            .expect("handler present")
    }

    async fn create_wallet_file(password: &str) -> (String, KeyPair, String) {
        let settings = Arc::new(ProtocolSettings::default());
        let path = temp_wallet_path();
        let wallet = Nep6Wallet::new(
            Some("rpc-wallet".to_string()),
            Some(path.clone()),
            settings.clone(),
        );
        let keypair = KeyPair::from_private_key(&[0x11u8; 32]).expect("keypair");
        let nep2 = keypair
            .to_nep2(password, settings.address_version)
            .expect("nep2");
        wallet
            .import_nep2(&nep2, password)
            .await
            .expect("import nep2");
        wallet.persist().expect("persist wallet");
        let address =
            WalletHelper::to_address(&keypair.get_script_hash(), settings.address_version);
        (path, keypair, address)
    }

    fn make_authenticated_server() -> RpcServer {
        make_authenticated_server_with_max_fee(RpcServerConfig::default().max_fee)
    }

    fn authenticated_config() -> RpcServerConfig {
        RpcServerConfig {
            rpc_user: "user".to_string(),
            rpc_pass: "pass".to_string(),
            ..Default::default()
        }
    }

    fn authenticated_config_with_max_fee(max_fee: i64) -> RpcServerConfig {
        RpcServerConfig {
            max_fee,
            ..authenticated_config()
        }
    }

    fn make_authenticated_server_with_max_fee(max_fee: i64) -> RpcServer {
        let system = if Handle::try_current().is_ok() {
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
        } else {
            let rt = Runtime::new().expect("runtime");
            let system = rt.block_on(async {
                NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
            });
            drop(rt);
            system
        };
        let config = authenticated_config_with_max_fee(max_fee);
        RpcServer::new(system, config)
    }

    fn mint_gas(
        store: &mut neo_core::persistence::StoreCache,
        settings: &ProtocolSettings,
        account: UInt160,
        amount: BigInt,
    ) {
        let snapshot = Arc::new(store.data_cache().clone());
        let mut container = Transaction::new();
        container.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
        container.add_witness(Witness::new());
        let script_container: Arc<dyn IVerifiable> = Arc::new(container);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(script_container),
            snapshot,
            None,
            settings.clone(),
            400_000_000,
            None,
        )
        .expect("engine");

        let gas = GasToken::new();
        gas.mint(&mut engine, &account, &amount, false)
            .expect("mint");
    }

    fn build_signed_transaction_custom(
        settings: &ProtocolSettings,
        keypair: &KeyPair,
        nonce: u32,
        system_fee: i64,
        network_fee: i64,
        script: Vec<u8>,
    ) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(network_fee);
        tx.set_system_fee(system_fee);
        tx.set_valid_until_block(1);
        tx.set_script(script);
        tx.set_signers(vec![Signer::new(
            keypair.get_script_hash(),
            WitnessScope::GLOBAL,
        )]);

        let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
        let signature = keypair.sign(&sign_data).expect("sign");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        let verification_script = keypair.get_verification_script();
        tx.set_witnesses(vec![Witness::new_with_scripts(
            invocation,
            verification_script,
        )]);
        tx
    }

    fn persist_transaction_record(store: &mut neo_core::persistence::StoreCache, tx: &Transaction) {
        const PREFIX_TRANSACTION: u8 = 0x0b;
        const RECORD_KIND_TRANSACTION: u8 = 0x01;

        let mut writer = BinaryWriter::new();
        writer
            .write_u8(RECORD_KIND_TRANSACTION)
            .expect("record kind");
        writer.write_u32(0).expect("block index");
        writer.write_u8(VMState::NONE as u8).expect("vm state");
        let tx_bytes = tx.to_bytes();
        writer.write_var_bytes(&tx_bytes).expect("tx bytes");

        let mut key_bytes = Vec::with_capacity(1 + 32);
        key_bytes.push(PREFIX_TRANSACTION);
        key_bytes.extend_from_slice(&tx.hash().to_bytes());
        let key = StorageKey::new(LedgerContract::ID, key_bytes);
        store.add(key, StorageItem::from_bytes(writer.to_bytes()));
        store.commit();
    }

    #[test]
    fn signature_contract_pubkey_roundtrip() {
        let private_key = [1u8; 32];
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("pubkey");
        let script = ContractHelper::signature_redeem_script(&public_key);
        let recovered = signature_contract_pubkey(&script).expect("parse pubkey");
        assert_eq!(recovered, public_key);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_wallet_and_dump_priv_key_roundtrip() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case("openwallet"))
            .expect("openwallet handler");
        let dump_handler = handlers
            .iter()
            .find(|handler| {
                handler
                    .descriptor()
                    .name
                    .eq_ignore_ascii_case("dumpprivkey")
            })
            .expect("dumpprivkey handler");
        let close_handler = handlers
            .iter()
            .find(|handler| {
                handler
                    .descriptor()
                    .name
                    .eq_ignore_ascii_case("closewallet")
            })
            .expect("closewallet handler");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        let result = (open_handler.callback())(&server, &params).expect("open wallet");
        assert_eq!(result.as_bool(), Some(true));
        assert!(server.wallet().is_some());

        let params = [Value::String(address)];
        let result = (dump_handler.callback())(&server, &params).expect("dump priv key");
        assert_eq!(result.as_str().expect("wif"), keypair.to_wif());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));
        assert!(server.wallet().is_none());

        fs::remove_file(path).ok();
    }

    #[test]
    fn close_wallet_returns_true_when_no_wallet_open() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let close_handler = find_handler(&handlers, "closewallet");

        assert!(server.wallet().is_none());
        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));
        assert!(server.wallet().is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_wallet_rejects_invalid_password() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case("openwallet"))
            .expect("openwallet handler");

        let params = [
            Value::String(path.clone()),
            Value::String("wrong".to_string()),
        ];
        let err = (open_handler.callback())(&server, &params).expect_err("invalid password");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::wallet_not_supported().code());
        assert_eq!(rpc_error.data(), Some("Invalid password."));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_wallet_rejects_missing_file() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");

        let path = temp_wallet_path();
        let params = [
            Value::String(path.clone()),
            Value::String("password".to_string()),
        ];
        let err = (open_handler.callback())(&server, &params).expect_err("missing wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::wallet_not_found().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_wallet_rejects_invalid_wallet_format() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");

        let path = temp_wallet_path();
        fs::write(&path, "{}").expect("write invalid wallet");

        let params = [
            Value::String(path.clone()),
            Value::String("password".to_string()),
        ];
        let err = (open_handler.callback())(&server, &params).expect_err("invalid wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::wallet_not_supported().code());

        fs::remove_file(path).ok();
    }

    #[test]
    fn get_new_address_adds_wallet_account() {
        let password = "rpc-pass";
        let rt = Runtime::new().expect("runtime");
        let (path, _keypair, _address) = rt.block_on(create_wallet_file(password));
        let system = rt.block_on(async {
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
        });
        drop(rt);
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let new_address_handler = find_handler(&handlers, "getnewaddress");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        let result = (open_handler.callback())(&server, &params).expect("open wallet");
        assert_eq!(result.as_bool(), Some(true));

        let result = (new_address_handler.callback())(&server, &[]).expect("get new address");
        let new_address = result.as_str().expect("address");
        let wallet = server.wallet().expect("wallet");
        let accounts = wallet.get_accounts();
        assert!(accounts
            .iter()
            .any(|account| account.address() == new_address));

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_wallet_balance_reports_balance_field() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let balance_handler = find_handler(&handlers, "getwalletbalance");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = NeoToken::new().hash().to_string();
        let params = [Value::String(asset)];
        let result = (balance_handler.callback())(&server, &params).expect("get wallet balance");
        let obj = result.as_object().expect("balance object");
        assert!(obj.get("balance").is_some());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_wallet_balance_rejects_invalid_asset_id() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let balance_handler = find_handler(&handlers, "getwalletbalance");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [Value::String("NotAValidAssetID".to_string())];
        let err = (balance_handler.callback())(&server, &params).expect_err("invalid asset id");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_wallet_unclaimed_gas_returns_string() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let gas_handler = find_handler(&handlers, "getwalletunclaimedgas");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let result = (gas_handler.callback())(&server, &[]).expect("get wallet unclaimed gas");
        assert!(result.as_str().is_some());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[test]
    fn import_priv_key_adds_account() {
        let password = "rpc-pass";
        let rt = Runtime::new().expect("runtime");
        let (path, _keypair, _address) = rt.block_on(create_wallet_file(password));
        let system = rt.block_on(async {
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start")
        });
        drop(rt);

        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let import_handler = find_handler(&handlers, "importprivkey");
        let list_handler = find_handler(&handlers, "listaddress");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let new_key = KeyPair::from_private_key(&[0x22u8; 32]).expect("keypair");
        let wif = new_key.to_wif();
        let expected_address = WalletHelper::to_address(
            &new_key.get_script_hash(),
            ProtocolSettings::default().address_version,
        );

        let params = [Value::String(wif)];
        let result = (import_handler.callback())(&server, &params).expect("import privkey");
        let obj = result.as_object().expect("account json");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(expected_address.as_str())
        );
        assert_eq!(obj.get("haskey").and_then(Value::as_bool), Some(true));
        assert_eq!(obj.get("watchonly").and_then(Value::as_bool), Some(false));

        let result = (list_handler.callback())(&server, &[]).expect("listaddress");
        let accounts = result.as_array().expect("account list");
        assert!(accounts
            .iter()
            .filter_map(|entry| entry.as_object())
            .any(|entry| entry.get("address").and_then(Value::as_str)
                == Some(expected_address.as_str())));

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn import_priv_key_rejects_invalid_wif() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let import_handler = find_handler(&handlers, "importprivkey");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [Value::String("ThisIsAnInvalidWIFString".to_string())];
        let err = (import_handler.callback())(&server, &params).expect_err("invalid wif");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn import_priv_key_returns_existing_account() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let import_handler = find_handler(&handlers, "importprivkey");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let wallet = server.wallet().expect("wallet");
        let existing = wallet
            .get_accounts()
            .into_iter()
            .find(|account| account.has_key())
            .expect("existing account");
        let existing_wif = existing.export_wif().expect("wif");
        let initial_count = wallet.get_accounts().len();

        let params = [Value::String(existing_wif)];
        let result = tokio::task::block_in_place(|| (import_handler.callback())(&server, &params))
            .expect("import existing");
        let obj = result.as_object().expect("account json");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(existing.address().as_str())
        );
        assert_eq!(obj.get("haskey").and_then(Value::as_bool), Some(true));
        assert_eq!(obj.get("watchonly").and_then(Value::as_bool), Some(false));
        if let Some(label) = existing.label() {
            assert_eq!(obj.get("label").and_then(Value::as_str), Some(label));
        } else {
            assert!(obj.get("label").is_some_and(Value::is_null));
        }

        let current_count = wallet.get_accounts().len();
        assert_eq!(current_count, initial_count);

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dump_priv_key_rejects_unknown_account() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let dump_handler = find_handler(&handlers, "dumpprivkey");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let other_key = KeyPair::from_private_key(&[0x33u8; 32]).expect("keypair");
        let other_address = WalletHelper::to_address(
            &other_key.get_script_hash(),
            ProtocolSettings::default().address_version,
        );
        let params = [Value::String(other_address)];
        let err = (dump_handler.callback())(&server, &params).expect_err("unknown account");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::unknown_account().code());
        let other_hash = other_key.get_script_hash().to_string();
        assert_eq!(rpc_error.data(), Some(other_hash.as_str()));

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dump_priv_key_rejects_invalid_address_format() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let dump_handler = find_handler(&handlers, "dumpprivkey");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [Value::String("NotAValidAddress".to_string())];
        let err = (dump_handler.callback())(&server, &params).expect_err("invalid address");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[test]
    fn cancel_transaction_requires_wallet() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let handler = find_handler(&handlers, "canceltransaction");
        let txid = UInt256::from([0x11u8; 32]).to_string();
        let address =
            WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
        let params = [
            Value::String(txid),
            Value::Array(vec![Value::String(address)]),
        ];

        let err = (handler.callback())(&server, &params).expect_err("no wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_rejects_invalid_txid() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [
            Value::String("invalid_txid".to_string()),
            Value::Array(vec![Value::String(address)]),
        ];
        let err = (handler.callback())(&server, &params).expect_err("invalid txid");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_rejects_empty_signers() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [
            Value::String(UInt256::from([0x22u8; 32]).to_string()),
            Value::Array(Vec::new()),
        ];
        let err = (handler.callback())(&server, &params).expect_err("empty signers");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::bad_request().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_returns_transaction_json() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let mut store = server.system().context().store_snapshot_cache();
        mint_gas(
            &mut store,
            server.system().settings(),
            keypair.get_script_hash(),
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let txid = UInt256::from([0x33u8; 32]).to_string();
        let params = [
            Value::String(txid),
            Value::Array(vec![Value::String(address.clone())]),
        ];
        let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect("canceltransaction");
        let obj = result.as_object().expect("tx json");
        assert_eq!(
            obj.get("sender").and_then(Value::as_str),
            Some(address.as_str())
        );
        let signers = obj
            .get("signers")
            .and_then(Value::as_array)
            .expect("signers");
        let signer = signers[0].as_object().expect("signer");
        assert_eq!(signer.get("scopes").and_then(Value::as_str), Some("None"));
        let attributes = obj
            .get("attributes")
            .and_then(Value::as_array)
            .expect("attributes");
        assert_eq!(
            attributes[0].get("type").and_then(Value::as_str),
            Some("Conflicts")
        );

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_rejects_invalid_signer_entry() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [
            Value::String(UInt256::from([0x66u8; 32]).to_string()),
            json!([{"account": "not-an-address"}]),
        ];
        let err = (handler.callback())(&server, &params).expect_err("invalid signer entry");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_rejects_confirmed_transaction() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let confirmed = build_signed_transaction_custom(
            server.system().settings(),
            &keypair,
            7,
            0,
            1,
            vec![OpCode::PUSH1 as u8],
        );
        let mut store = server.system().context().store_snapshot_cache();
        persist_transaction_record(&mut store, &confirmed);

        let params = [
            Value::String(confirmed.hash().to_string()),
            Value::Array(vec![Value::String(address)]),
        ];
        let err = (handler.callback())(&server, &params).expect_err("confirmed tx");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::already_exists().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_rejects_invalid_extra_fee() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let mut store = server.system().context().store_snapshot_cache();
        mint_gas(
            &mut store,
            server.system().settings(),
            WalletHelper::to_script_hash(&address, server.system().settings().address_version)
                .expect("script hash"),
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [
            Value::String(UInt256::from([0x44u8; 32]).to_string()),
            Value::Array(vec![Value::String(address)]),
            Value::String("0".to_string()),
        ];
        let err = (handler.callback())(&server, &params).expect_err("invalid extra fee");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_rejects_wallet_fee_limit() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;
        let server = make_authenticated_server_with_max_fee(1);
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let mut store = server.system().context().store_snapshot_cache();
        mint_gas(
            &mut store,
            server.system().settings(),
            keypair.get_script_hash(),
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [
            Value::String(UInt256::from([0x77u8; 32]).to_string()),
            Value::Array(vec![Value::String(address)]),
            Value::String("100".to_string()),
        ];
        let err = (handler.callback())(&server, &params).expect_err("wallet fee limit");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::wallet_fee_limit().code());
        assert!(rpc_error.data().is_some());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_applies_extra_fee() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;
        let server = make_authenticated_server_with_max_fee(1_000_000_000);
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let mut store = server.system().context().store_snapshot_cache();
        mint_gas(
            &mut store,
            server.system().settings(),
            keypair.get_script_hash(),
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let txid = UInt256::from([0x55u8; 32]);
        let conflict = TransactionAttribute::Conflicts(Conflicts::new(txid));
        let signers = vec![Signer::new(keypair.get_script_hash(), WitnessScope::NONE)];
        let snapshot = server.system().store_cache();
        let snapshot_arc = Arc::new(snapshot.data_cache().clone());
        let base_tx = Helper::make_transaction(
            server.wallet().expect("wallet").as_ref(),
            snapshot_arc.as_ref(),
            &[OpCode::RET as u8],
            Some(signers[0].account),
            Some(&signers),
            Some(std::slice::from_ref(&conflict)),
            server.system().settings(),
            None,
            server.settings().max_gas_invoke,
        )
        .expect("base cancel tx");
        let base_fee = base_tx.network_fee();

        let params = [
            Value::String(txid.to_string()),
            Value::Array(vec![Value::String(address)]),
            Value::String("1".to_string()),
        ];
        let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect("canceltransaction");
        let net_fee = result
            .get("netfee")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<i64>().ok())
            .expect("netfee");
        let expected_extra = 10_i64.pow(GasToken::new().decimals() as u32);
        assert_eq!(net_fee, base_fee + expected_extra);

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_transaction_bumps_fee_for_mempool_conflict() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;
        let server = make_authenticated_server_with_max_fee(1_000_000_000);
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "canceltransaction");
        let close_handler = find_handler(&handlers, "closewallet");

        let mut store = server.system().context().store_snapshot_cache();
        mint_gas(
            &mut store,
            server.system().settings(),
            keypair.get_script_hash(),
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let conflict_tx = build_signed_transaction_custom(
            server.system().settings(),
            &keypair,
            1,
            0,
            200_000_000,
            vec![OpCode::PUSH1 as u8],
        );
        let txid = conflict_tx.hash();
        let store_cache = server.system().store_cache();
        let verify = server.system().mempool().lock().try_add(
            conflict_tx.clone(),
            store_cache.data_cache(),
            server.system().settings(),
        );
        assert_eq!(verify, VerifyResult::Succeed);

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [
            Value::String(txid.to_string()),
            Value::Array(vec![Value::String(address)]),
        ];
        let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect("canceltransaction");
        let net_fee = result
            .get("netfee")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<i64>().ok())
            .expect("netfee");
        assert_eq!(net_fee, conflict_tx.network_fee() + 1);

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }
    #[test]
    fn wallet_methods_require_open_wallet() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let address =
            WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
        let asset = GasToken::new().hash().to_string();
        let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
        let wif = keypair.to_wif();

        let cases = vec![
            ("dumpprivkey", vec![Value::String(address.clone())]),
            ("getnewaddress", vec![]),
            ("getwalletbalance", vec![Value::String(asset.clone())]),
            ("getwalletunclaimedgas", vec![]),
            ("importprivkey", vec![Value::String(wif.clone())]),
            ("listaddress", vec![]),
        ];

        for (name, params) in cases {
            let handler = find_handler(&handlers, name);
            let err = (handler.callback())(&server, &params).expect_err("no wallet");
            let rpc_error: RpcError = err.into();
            assert_eq!(
                rpc_error.code(),
                RpcError::no_opened_wallet().code(),
                "{} should require a wallet",
                name
            );
        }
    }

    #[test]
    fn send_from_requires_wallet() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let send_handler = find_handler(&handlers, "sendfrom");
        let address =
            WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
        let asset = GasToken::new().hash().to_string();
        let params = [
            Value::String(asset),
            Value::String(address.clone()),
            Value::String(address),
            Value::String("1".to_string()),
        ];

        let err = (send_handler.callback())(&server, &params).expect_err("no wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_from_returns_transaction_json() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system.clone(), authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let send_handler = find_handler(&handlers, "sendfrom");
        let close_handler = find_handler(&handlers, "closewallet");

        let mut store = system.context().store_snapshot_cache();
        mint_gas(
            &mut store,
            system.settings(),
            keypair.get_script_hash(),
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = GasToken::new().hash().to_string();
        let params = [
            Value::String(asset),
            Value::String(address.clone()),
            Value::String(address.clone()),
            Value::String("1".to_string()),
        ];
        let result = tokio::task::block_in_place(|| (send_handler.callback())(&server, &params))
            .expect("sendfrom");
        let obj = result.as_object().expect("tx json");
        assert_eq!(obj.len(), 12);
        assert_eq!(
            obj.get("sender").and_then(Value::as_str),
            Some(address.as_str())
        );

        let signers = obj
            .get("signers")
            .and_then(Value::as_array)
            .expect("signers");
        assert_eq!(signers.len(), 1);
        let signer = signers[0].as_object().expect("signer");
        let expected_account = keypair.get_script_hash().to_string();
        assert_eq!(
            signer.get("account").and_then(Value::as_str),
            Some(expected_account.as_str())
        );
        assert_eq!(
            signer.get("scopes").and_then(Value::as_str),
            Some("CalledByEntry")
        );

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_from_returns_invalid_request_without_funds() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let send_handler = find_handler(&handlers, "sendfrom");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = GasToken::new().hash().to_string();
        let params = [
            Value::String(asset),
            Value::String(address.clone()),
            Value::String(address.clone()),
            Value::String("1".to_string()),
        ];
        let err = (send_handler.callback())(&server, &params).expect_err("insufficient funds");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_request().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[test]
    fn send_to_address_requires_wallet() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let handler = find_handler(&handlers, "sendtoaddress");
        let address =
            WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
        let asset = GasToken::new().hash().to_string();
        let params = [
            Value::String(asset),
            Value::String(address),
            Value::String("1".to_string()),
        ];

        let err = (handler.callback())(&server, &params).expect_err("no wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_to_address_rejects_invalid_asset_id() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendtoaddress");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let address =
            WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
        let params = [
            Value::String("NotAnAssetId".to_string()),
            Value::String(address),
            Value::String("1".to_string()),
        ];

        let err = (handler.callback())(&server, &params).expect_err("invalid asset");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_to_address_rejects_invalid_to_address() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendtoaddress");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = GasToken::new().hash().to_string();
        let params = [
            Value::String(asset),
            Value::String("NotAnAddress".to_string()),
            Value::String("1".to_string()),
        ];

        let err = (handler.callback())(&server, &params).expect_err("invalid address");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_to_address_rejects_non_positive_amount() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendtoaddress");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = GasToken::new().hash().to_string();
        for amount in ["-1", "0"] {
            let params = [
                Value::String(asset.clone()),
                Value::String(address.clone()),
                Value::String(amount.to_string()),
            ];
            let err = (handler.callback())(&server, &params).expect_err("invalid amount");
            let rpc_error: RpcError = err.into();
            assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        }

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_to_address_reports_invalid_operation_on_insufficient_funds() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendtoaddress");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = GasToken::new().hash().to_string();
        let params = [
            Value::String(asset),
            Value::String(address),
            Value::String("100000000000000000".to_string()),
        ];
        let err = (handler.callback())(&server, &params).expect_err("insufficient funds");
        assert_eq!(err.code(), INVALID_OPERATION_HRESULT);

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[test]
    fn send_many_requires_wallet() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let handler = find_handler(&handlers, "sendmany");
        let address =
            WalletHelper::to_address(&UInt160::zero(), server.system().settings().address_version);
        let asset = GasToken::new().hash().to_string();
        let outputs = json!([{
            "asset": asset,
            "value": "1",
            "address": address.clone()
        }]);
        let params = [Value::String(address), outputs];

        let err = (handler.callback())(&server, &params).expect_err("no wallet");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::no_opened_wallet().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_many_rejects_invalid_from() {
        let password = "rpc-pass";
        let (path, _keypair, _address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendmany");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = GasToken::new().hash().to_string();
        let outputs = json!([{
            "asset": asset,
            "value": "1",
            "address": WalletHelper::to_address(
                &UInt160::zero(),
                server.system().settings().address_version,
            )
        }]);
        let params = [Value::String("NotAnAddress".to_string()), outputs];

        let err = (handler.callback())(&server, &params).expect_err("invalid from");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_many_rejects_empty_outputs() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendmany");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [Value::String(address), Value::Array(vec![])];
        let err = (handler.callback())(&server, &params).expect_err("empty outputs");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(
            rpc_error
                .data()
                .unwrap_or_default()
                .contains("Argument 'to' can't be empty"),
            "unexpected error message: {:?}",
            rpc_error.data()
        );

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_many_rejects_invalid_outputs_type() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendmany");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let params = [
            Value::String(address),
            Value::String("not-an-array".to_string()),
        ];
        let err = (handler.callback())(&server, &params).expect_err("invalid outputs");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(
            rpc_error
                .data()
                .unwrap_or_default()
                .contains("Invalid 'to' parameter"),
            "unexpected error message: {:?}",
            rpc_error.data()
        );

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_many_rejects_non_positive_amount() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendmany");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset_id = GasToken::new().hash();
        let asset = asset_id.to_string();
        for amount in ["-1", "0"] {
            let outputs = json!([{
                "asset": asset.clone(),
                "value": amount,
                "address": address.clone()
            }]);
            let params = [Value::String(address.clone()), outputs];
            let err = (handler.callback())(&server, &params).expect_err("invalid amount");
            let rpc_error: RpcError = err.into();
            assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
            assert!(
                rpc_error
                    .data()
                    .unwrap_or_default()
                    .contains(&format!("Amount of '{}' can't be negative.", asset_id)),
                "unexpected error message: {:?}",
                rpc_error.data()
            );
        }

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_many_returns_transaction_json() {
        let password = "rpc-pass";
        let (path, keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system.clone(), authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendmany");
        let close_handler = find_handler(&handlers, "closewallet");

        let mut store = system.context().store_snapshot_cache();
        mint_gas(
            &mut store,
            system.settings(),
            keypair.get_script_hash(),
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let asset = GasToken::new().hash().to_string();
        let outputs = json!([{
            "asset": asset,
            "value": "1",
            "address": address.clone()
        }]);
        let params = [Value::String(address.clone()), outputs];
        let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect("sendmany");
        let obj = result.as_object().expect("tx json");
        assert_eq!(obj.len(), 12);
        assert_eq!(
            obj.get("sender").and_then(Value::as_str),
            Some(address.as_str())
        );

        let signers = obj
            .get("signers")
            .and_then(Value::as_array)
            .expect("signers");
        assert_eq!(signers.len(), 1);
        let signer = signers[0].as_object().expect("signer");
        let expected_account = keypair.get_script_hash().to_string();
        assert_eq!(
            signer.get("account").and_then(Value::as_str),
            Some(expected_account.as_str())
        );
        assert_eq!(
            signer.get("scopes").and_then(Value::as_str),
            Some("CalledByEntry")
        );

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_many_reports_invalid_operation_on_insufficient_funds() {
        let password = "rpc-pass";
        let (path, _keypair, address) = create_wallet_file(password).await;
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, authenticated_config());
        let handlers = RpcServerWallet::register_handlers();
        let open_handler = find_handler(&handlers, "openwallet");
        let handler = find_handler(&handlers, "sendmany");
        let close_handler = find_handler(&handlers, "closewallet");

        let params = [
            Value::String(path.clone()),
            Value::String(password.to_string()),
        ];
        (open_handler.callback())(&server, &params).expect("open wallet");

        let outputs = json!([{
            "asset": GasToken::new().hash().to_string(),
            "value": "100000000000000000",
            "address": address.clone()
        }]);
        let params = [Value::String(address), outputs];
        let err = (handler.callback())(&server, &params).expect_err("insufficient funds");
        assert_eq!(err.code(), INVALID_OPERATION_HRESULT);

        let result = (close_handler.callback())(&server, &[]).expect("close wallet");
        assert_eq!(result.as_bool(), Some(true));

        fs::remove_file(path).ok();
    }

    #[test]
    fn calculate_network_fee_requires_payload() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let handler = find_handler(&handlers, "calculatenetworkfee");

        let err = (handler.callback())(&server, &[]).expect_err("missing payload");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    #[test]
    fn calculate_network_fee_returns_network_fee() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let handler = find_handler(&handlers, "calculatenetworkfee");

        let settings = ProtocolSettings::default();
        let keypair = KeyPair::from_private_key(&[0x66u8; 32]).expect("keypair");
        let tx = build_signed_transaction_custom(
            &settings,
            &keypair,
            1,
            0,
            0,
            vec![OpCode::PUSH1 as u8],
        );
        let payload = BASE64_STANDARD.encode(tx.to_bytes());

        let params = [Value::String(payload)];
        let result = (handler.callback())(&server, &params).expect("network fee");
        let obj = result.as_object().expect("network fee object");
        let fee = obj
            .get("networkfee")
            .and_then(Value::as_str)
            .expect("network fee");
        assert!(fee.parse::<i64>().is_ok());
    }

    #[test]
    fn calculate_network_fee_rejects_invalid_payload() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let handler = find_handler(&handlers, "calculatenetworkfee");
        let params = [Value::String("invalid_base64".to_string())];

        let err = (handler.callback())(&server, &params).expect_err("invalid payload");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    #[test]
    fn calculate_network_fee_rejects_invalid_transaction_bytes() {
        let server = make_authenticated_server();
        let handlers = RpcServerWallet::register_handlers();
        let handler = find_handler(&handlers, "calculatenetworkfee");

        let payload = BASE64_STANDARD.encode([0x01u8, 0x02, 0x03]);
        let params = [Value::String(payload)];
        let err = (handler.callback())(&server, &params).expect_err("invalid tx bytes");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }
}
