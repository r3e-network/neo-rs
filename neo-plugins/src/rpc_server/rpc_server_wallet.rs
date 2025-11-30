//! RPC wallet endpoints mirroring RpcServer.Wallet.cs.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::big_decimal::BigDecimal;
use neo_core::cryptography::crypto_utils::ECPoint;
use neo_core::ledger::{RelayResult, VerifyResult};
use neo_core::neo_system::TransactionRouterMessage;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::prelude::Serializable;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::contract_parameters_context::ContractParametersContext;
use neo_core::smart_contract::native::{
    GasToken, LedgerContract, NativeContract, NeoToken, PolicyContract,
};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::{
    AssetDescriptor, Helper, KeyPair, Nep6Wallet, TransferOutput, Wallet as CoreWallet,
    WalletAccount, WalletError, WalletResult,
};
use neo_core::{witness_scope::WitnessScope, UInt160};
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

use crate::rpc_server::rpc_error::RpcError;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::rpc_server::{RpcHandler, RpcServer};

pub struct RpcServerWallet;

impl RpcServerWallet {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("closewallet", Self::close_wallet),
            Self::handler("dumpprivkey", Self::dump_priv_key),
            Self::handler("getnewaddress", Self::get_new_address),
            Self::handler("getwalletbalance", Self::get_wallet_balance),
            Self::handler("getwalletunclaimedgas", Self::get_wallet_unclaimed_gas),
            Self::handler("importprivkey", Self::import_priv_key),
            Self::handler("listaddress", Self::list_address),
            Self::handler("openwallet", Self::open_wallet),
            Self::handler("calculatenetworkfee", Self::calculate_network_fee),
            Self::handler("sendfrom", Self::send_from),
            Self::handler("sendtoaddress", Self::send_to_address),
            Self::handler("sendmany", Self::send_many),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
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
                RpcError::unknown_account().with_data(format!("{} is watch-only", script_hash)),
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
        let private_key = key_pair.private_key();
        let wallet_clone = Arc::clone(&wallet);
        let key_bytes = private_key;
        let account = Self::await_wallet_future(Box::pin(async move {
            wallet_clone.create_account(&key_bytes).await
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
        let wallet = Self::require_wallet(server)?;
        let wallet_clone = Arc::clone(&wallet);
        let privkey_value = privkey.clone();
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
                RpcError::invalid_params().with_data(format!("Invalid transaction: {}", err)),
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
        Self::process_transfer(
            server,
            asset,
            Some(from_hash),
            to_hash,
            amount_text,
            signers.as_deref(),
        )
    }

    fn send_to_address(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
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
        Self::process_transfer(
            server,
            asset,
            None,
            to_hash,
            amount_text,
            signers.as_deref(),
        )
    }

    fn send_many(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
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

        let outputs_value = params
            .get(index)
            .ok_or_else(|| Self::invalid_params("sendmany missing outputs"))?;
        let outputs_array = outputs_value
            .as_array()
            .ok_or_else(|| Self::invalid_params("outputs must be an array"))?;

        let signers = if params.len() > index + 1 {
            Some(Self::parse_signers(server, &params[index + 1])?)
        } else {
            None
        };

        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let descriptor_cache = |asset: &UInt160| {
            AssetDescriptor::new(store.data_cache(), server.system().settings(), *asset)
        };

        let mut transfers = Vec::new();
        for (i, entry) in outputs_array.iter().enumerate() {
            let obj = entry
                .as_object()
                .ok_or_else(|| Self::invalid_params(format!("invalid output at {}", i)))?;
            let asset_str = obj
                .get("asset")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Self::invalid_params("asset missing"))?;
            let asset = UInt160::from_str(asset_str)
                .map_err(|e| Self::invalid_params(format!("invalid asset {}: {}", asset_str, e)))?;
            let descriptor =
                descriptor_cache(&asset).map_err(|e| Self::invalid_params(e.to_string()))?;
            let value_str = obj
                .get("value")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Self::invalid_params("value missing"))?;
            let (ok, value) = BigDecimal::try_parse(value_str, descriptor.decimals);
            if !ok || value.sign() <= 0 {
                return Err(Self::invalid_params("value must be positive"));
            }
            let address_str = obj
                .get("address")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Self::invalid_params("address missing"))?;
            let to_hash = Self::parse_script_hash(server, address_str)?;
            transfers.push(TransferOutput {
                asset_id: asset,
                value,
                script_hash: to_hash,
                data: None,
            });
        }

        let tx_json = Self::build_and_relay(server, &wallet, &transfers, from, signers.as_deref())?;
        Ok(tx_json)
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
                    .with_data(format!("invalid UInt160 '{}': {}", text, err)),
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
            .map(|value| value.to_string())
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
                RpcException::from(RpcError::unknown_account().with_data(format!("{}", hash)))
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
                .map(|label| Value::String(label.to_string()))
                .unwrap_or(Value::Null),
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

        builder.emit_push_int(flags.bits() as i64);
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
        let mut tx = Helper::make_transfer_transaction(
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

        // Build contract parameter context and add signatures from available keys
        let mut context = ContractParametersContext::new_with_type(
            snapshot_arc.clone(),
            tx.clone(),
            server.system().settings().network,
            Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
        );
        for signer in tx.signers() {
            if let Some(account) = wallet.get_account(&signer.account) {
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
                            let pub_key = ECPoint::new(key.compressed_public_key());
                            let _ = context.add_signature(contract.clone(), pub_key, signature);
                        }
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
                .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64);
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
                    snapshot_arc.clone(),
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
        F: FnOnce(akka::ActorRef) -> Result<(), RpcException>,
    {
        use akka::{Actor, ActorContext, ActorResult, Props};
        use async_trait::async_trait;
        use neo_core::ledger::RelayResult;

        struct RelayResponder {
            tx: std::sync::Arc<std::sync::Mutex<Option<std::sync::mpsc::Sender<RelayResult>>>>,
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
                    if let Some(sender) = self.tx.lock().unwrap().take() {
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
            tx: std::sync::Arc::new(std::sync::Mutex::new(Some(tx))),
        };
        let props = Props::new(move || RelayResponder {
            tx: std::sync::Arc::clone(&responder.tx),
        });
        let actor_ref = actor_system
            .actor_of(props, format!("relay_responder_{}", Uuid::new_v4()))
            .map_err(|err| Self::internal_error(err.to_string()))?;

        send(actor_ref.clone())?;

        let result = rx
            .recv()
            .map_err(|err| Self::internal_error(err.to_string()))?;
        Ok(result)
    }
}
