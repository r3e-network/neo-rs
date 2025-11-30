//! Smart contract RPC endpoints (`RpcServer.SmartContract.cs` parity subset).

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_core::big_decimal::BigDecimal;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::store_cache::StoreCache;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter::{ContractParameter, ContractParameterValue};
use neo_core::smart_contract::contract_parameter_type::ContractParameterType;
use neo_core::smart_contract::native::{ledger_contract::LedgerContract, GasToken, NeoToken};
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::{Wallet, WalletAccount};
use neo_core::UInt160;
use neo_json::JToken;
use num_bigint::BigInt;
use rand::random;
use serde_json::{json, Map, Number as JsonNumber, Value};
use uuid::Uuid;

use crate::rpc_server::diagnostic::Diagnostic;
use crate::rpc_server::model::signers_and_witnesses::SignersAndWitnesses;
use crate::rpc_server::parameter_converter::{ConversionContext, ParameterConverter};
use crate::rpc_server::rpc_error::RpcError;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::rpc_server::{RpcHandler, RpcServer};
use crate::rpc_server::session::Session;
use crate::rpc_server::tree_node::TreeNode;
use neo_vm::stack_item::StackItem;

const TRANSACTION_TYPE_NAME: &str = "Neo.Network.P2P.Payloads.Transaction";

pub struct RpcServerSmartContract;

enum WalletInvocationOutcome {
    Signed(Vec<u8>),
    Pending(Value),
}

#[derive(Clone)]
struct PendingSignatureItem {
    account: UInt160,
    script: Option<Vec<u8>>,
    parameter_types: Vec<ContractParameterType>,
}

impl RpcServerSmartContract {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("invokefunction", Self::invoke_function),
            Self::handler("invokescript", Self::invoke_script),
            Self::handler("invokecontractverify", Self::invoke_contract_verify),
            Self::handler("traverseiterator", Self::traverse_iterator),
            Self::handler("terminatesession", Self::terminate_session),
            Self::handler("getunclaimedgas", Self::get_unclaimed_gas),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
    }

    fn invoke_function(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let script_hash = Self::expect_string_param(params, 0, "invokefunction")?;
        let script_hash = UInt160::from_str(&script_hash)
            .map_err(|err| Self::invalid_params(format!("invalid script hash: {}", err)))?;

        let operation = Self::expect_string_param(params, 1, "invokefunction")?;
        let parameters = Self::parse_contract_parameters(params.get(2))?;
        let (signers, witnesses) = Self::parse_signers_and_witnesses(server, params.get(3))?;
        let use_diagnostic = params
            .get(4)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let script = Self::build_dynamic_call_script(script_hash, &operation, &parameters)?;
        Self::execute_script(server, script, signers, witnesses, use_diagnostic)
    }

    fn invoke_script(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let script_base64 = Self::expect_string_param(params, 0, "invokescript")?;
        let script = BASE64_STANDARD
            .decode(script_base64.trim())
            .map_err(|_| Self::invalid_params("invalid script payload"))?;
        let (signers, witnesses) = Self::parse_signers_and_witnesses(server, params.get(1))?;
        let use_diagnostic = params
            .get(2)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        Self::execute_script(server, script, signers, witnesses, use_diagnostic)
    }

    /// Invokes a contract's verify method.
    /// Matches C# RpcServer.SmartContract.InvokeContractVerify exactly.
    fn invoke_contract_verify(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let script_hash = Self::expect_string_param(params, 0, "invokecontractverify")?;
        let script_hash = UInt160::from_str(&script_hash)
            .map_err(|err| Self::invalid_params(format!("invalid script hash: {}", err)))?;

        let parameters = Self::parse_contract_parameters(params.get(1))?;
        let (signers, witnesses) = Self::parse_signers_and_witnesses(server, params.get(2))?;

        // Build script that calls the verify method
        let script = Self::build_dynamic_call_script(script_hash, "verify", &parameters)?;
        Self::execute_script(server, script, signers, witnesses, false)
    }

    fn traverse_iterator(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if !server.session_enabled() {
            return Err(RpcException::from(RpcError::sessions_disabled()));
        }
        let session_id = Self::expect_uuid_param(params, 0, "traverseiterator")?;
        let iterator_id = Self::expect_uuid_param(params, 1, "traverseiterator")?;
        let count = Self::expect_u32_param(params, 2, "traverseiterator")?;
        if (count as usize) > server.settings().max_iterator_result_items {
            return Err(Self::invalid_params(format!(
                "Invalid iterator items count {}",
                count
            )));
        }
        server.purge_expired_sessions();
        let result = server
            .with_session_mut(&session_id, |session| {
                session.reset_expiration();
                match session.traverse_iterator(&iterator_id, count as usize) {
                    Ok(items) => {
                        let mut session_ref = Some(session);
                        let mut values = Vec::new();
                        for item in items {
                            values
                                .push(Self::stack_item_to_json(&item, session_ref.as_deref_mut())?);
                        }
                        Ok(Value::Array(values))
                    }
                    Err(message) if message == "Unknown iterator" => {
                        Err(RpcException::from(RpcError::unknown_iterator()))
                    }
                    Err(message) => Err(Self::internal_error(message)),
                }
            })
            .ok_or_else(|| RpcException::from(RpcError::unknown_session()))??;

        Ok(result)
    }

    fn terminate_session(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if !server.session_enabled() {
            return Err(RpcException::from(RpcError::sessions_disabled()));
        }
        let session_id = Self::expect_uuid_param(params, 0, "terminatesession")?;
        server.purge_expired_sessions();
        if !server.terminate_session(&session_id) {
            return Err(RpcException::from(RpcError::unknown_session()));
        }
        Ok(Value::Bool(true))
    }

    fn get_unclaimed_gas(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let address_text = Self::expect_string_param(params, 0, "getunclaimedgas")?;
        let version = server.system().settings().address_version;
        let script_hash = if let Ok(hash) = UInt160::from_str(&address_text) {
            hash
        } else {
            WalletHelper::to_script_hash(&address_text, version).map_err(Self::invalid_params)?
        };

        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let height = ledger
            .current_index(&store)
            .map_err(|err| Self::internal_error(err.to_string()))?
            .saturating_add(1);
        let neo = NeoToken::new();
        let unclaimed = neo
            .unclaimed_gas(&store, &script_hash, height)
            .map_err(|err| Self::internal_error(err.to_string()))?;

        Ok(json!({
            "address": script_hash.to_string(),
            "unclaimed": BigDecimal::new(unclaimed, neo.decimals()).to_string()
        }))
    }

    fn execute_script(
        server: &RpcServer,
        script: Vec<u8>,
        signers: Option<Vec<Signer>>,
        witnesses: Option<Vec<Witness>>,
        use_diagnostic: bool,
    ) -> Result<Value, RpcException> {
        let diagnostic = if use_diagnostic {
            Some(Diagnostic::new())
        } else {
            None
        };
        let mut session = Session::new(
            server.system(),
            script,
            signers.clone(),
            witnesses,
            server.settings().max_gas_invoke,
            diagnostic,
        )
        .map_err(Self::internal_error)?;
        let engine_state = format!("{:?}", session.engine().state());
        let system_fee = session.engine().fee_consumed();
        let gas_consumed = system_fee.to_string();
        let exception_value = session
            .engine()
            .fault_exception()
            .map(|msg| Value::String(msg.to_string()))
            .unwrap_or(Value::Null);
        let notifications_snapshot = session.engine().notifications().to_vec();
        let stack_snapshot: Vec<StackItem> =
            session.engine().result_stack().iter().cloned().collect();
        let diagnostics_snapshot = session.diagnostic().map(|diag| {
            let invocation = Self::diagnostic_invocation_to_json(diag);
            let storage = Self::diagnostic_storage_changes(session.engine());
            (invocation, storage)
        });

        let mut result = Map::new();
        result.insert(
            "script".to_string(),
            Value::String(BASE64_STANDARD.encode(session.script())),
        );
        result.insert("state".to_string(), Value::String(engine_state));
        result.insert("gasconsumed".to_string(), Value::String(gas_consumed));
        result.insert("exception".to_string(), exception_value);
        let notifications = {
            let mut session_ref = Some(&mut session);
            let mut entries = Vec::new();
            for notification in notifications_snapshot.iter() {
                entries.push(Self::notification_to_json(
                    notification,
                    session_ref.as_deref_mut(),
                )?);
            }
            entries
        };
        result.insert("notifications".to_string(), Value::Array(notifications));
        let stack_items = {
            let mut session_ref = Some(&mut session);
            let mut entries = Vec::new();
            for item in stack_snapshot.iter() {
                entries.push(Self::stack_item_to_json(item, session_ref.as_deref_mut())?);
            }
            entries
        };
        result.insert("stack".to_string(), Value::Array(stack_items));

        if let Some((invocation, storage)) = diagnostics_snapshot {
            result.insert(
                "diagnostics".to_string(),
                json!({
                    "invokedcontracts": invocation,
                    "storagechanges": storage,
                }),
            );
        }

        Self::process_invoke_with_wallet(
            server,
            &mut result,
            session.script(),
            signers.as_ref(),
            session.snapshot(),
            system_fee,
        );

        if server.session_enabled() && session.has_iterators() {
            server.purge_expired_sessions();
            let session_id = server.store_session(session);
            result.insert("session".to_string(), Value::String(session_id.to_string()));
        }

        Ok(Value::Object(result))
    }

    fn process_invoke_with_wallet(
        server: &RpcServer,
        result: &mut Map<String, Value>,
        script: &[u8],
        signers: Option<&Vec<Signer>>,
        snapshot: &StoreCache,
        system_fee: i64,
    ) {
        let signers = match signers {
            Some(list) if !list.is_empty() => list,
            _ => return,
        };
        let wallet = match server.wallet() {
            Some(wallet) => wallet,
            None => return,
        };

        match Self::build_and_sign_transaction(
            server, script, signers, snapshot, system_fee, wallet,
        ) {
            Ok(WalletInvocationOutcome::Signed(bytes)) => {
                result.insert(
                    "tx".to_string(),
                    Value::String(BASE64_STANDARD.encode(bytes)),
                );
            }
            Ok(WalletInvocationOutcome::Pending(context)) => {
                result.insert("pendingsignature".to_string(), context);
            }
            Err(message) => {
                result.insert("exception".to_string(), Value::String(message));
            }
        }
    }

    fn build_and_sign_transaction(
        server: &RpcServer,
        script: &[u8],
        signers: &[Signer],
        snapshot: &StoreCache,
        system_fee: i64,
        wallet: Arc<dyn Wallet>,
    ) -> Result<WalletInvocationOutcome, String> {
        let rpc_settings = server.settings().clone();
        let system = server.system();
        let protocol_settings = system.settings();
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(random());
        tx.set_script(script.to_vec());
        tx.set_signers(signers.to_vec());
        tx.set_attributes(Vec::<TransactionAttribute>::new());

        let ledger = LedgerContract::new();
        let valid_until = ledger
            .current_index(snapshot)
            .map_err(|err| err.to_string())?
            .saturating_add(protocol_settings.max_valid_until_block_increment);
        tx.set_valid_until_block(valid_until);
        tx.set_system_fee(system_fee);

        let data_cache = snapshot.data_cache();
        let network_fee = WalletHelper::calculate_network_fee_with_wallet(
            &tx,
            data_cache,
            protocol_settings,
            Some(wallet.as_ref()),
            rpc_settings.max_gas_invoke,
        )?;
        tx.set_network_fee(network_fee);

        let required_fee = BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee());
        let gas_token = GasToken::new();
        let sender = signers[0].account;
        let available = gas_token.balance_of_snapshot(snapshot, &sender);
        if available < required_fee {
            return Err("Insufficient GAS balance to pay system and network fees.".to_string());
        }

        let mut tx_clone = tx.clone();
        let pending = Self::sign_transaction_with_wallet(&mut tx_clone, signers, &wallet);
        if pending.is_empty() {
            Ok(WalletInvocationOutcome::Signed(tx_clone.to_bytes()))
        } else {
            let context =
                Self::build_pending_context(&tx_clone, pending, protocol_settings.network);
            Ok(WalletInvocationOutcome::Pending(context))
        }
    }

    fn sign_transaction_with_wallet(
        tx: &mut Transaction,
        signers: &[Signer],
        wallet: &Arc<dyn Wallet>,
    ) -> Vec<PendingSignatureItem> {
        let mut pending = Vec::new();
        for signer in signers {
            match wallet.get_account(&signer.account) {
                Some(account) if account.has_key() => match account.create_witness(tx) {
                    Ok(witness) => tx.add_witness(witness),
                    Err(_) => pending.push(Self::build_pending_item(signer.account, Some(account))),
                },
                Some(account) => {
                    pending.push(Self::build_pending_item(signer.account, Some(account)))
                }
                None => pending.push(Self::build_pending_item(signer.account, None)),
            }
        }
        pending
    }

    fn build_pending_context(
        tx: &Transaction,
        pending: Vec<PendingSignatureItem>,
        network: u32,
    ) -> Value {
        let mut context = Map::new();
        context.insert(
            "type".to_string(),
            Value::String(TRANSACTION_TYPE_NAME.to_string()),
        );
        context.insert("hash".to_string(), Value::String(tx.hash().to_string()));
        context.insert(
            "data".to_string(),
            Value::String(BASE64_STANDARD.encode(tx.get_hash_data())),
        );

        let mut items = Map::new();
        for entry in pending {
            let mut obj = Map::new();
            obj.insert(
                "script".to_string(),
                entry
                    .script
                    .map(|bytes| Value::String(BASE64_STANDARD.encode(bytes)))
                    .unwrap_or(Value::Null),
            );

            let parameters = entry
                .parameter_types
                .into_iter()
                .map(|param_type| {
                    json!({
                        "type": param_type.as_str(),
                        "value": Value::Null,
                    })
                })
                .collect();
            obj.insert("parameters".to_string(), Value::Array(parameters));
            obj.insert("signatures".to_string(), Value::Object(Map::new()));

            items.insert(entry.account.to_string(), Value::Object(obj));
        }

        context.insert("items".to_string(), Value::Object(items));
        context.insert(
            "network".to_string(),
            Value::Number(JsonNumber::from(network)),
        );
        Value::Object(context)
    }

    fn build_pending_item(
        account: UInt160,
        wallet_account: Option<Arc<dyn WalletAccount>>,
    ) -> PendingSignatureItem {
        if let Some(account_ref) = wallet_account {
            if let Some(contract) = account_ref.contract() {
                return PendingSignatureItem {
                    account,
                    script: Some(contract.script.clone()),
                    parameter_types: contract.parameter_list.clone(),
                };
            }
        }

        PendingSignatureItem {
            account,
            script: None,
            parameter_types: Vec::new(),
        }
    }

    fn parse_contract_parameters(
        arg: Option<&Value>,
    ) -> Result<Vec<ContractParameter>, RpcException> {
        match arg {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| ContractParameter::from_json(value).map_err(Self::invalid_params))
                .collect(),
            Some(_) => Err(Self::invalid_params("args must be an array")),
        }
    }

    #[allow(clippy::type_complexity)]
    fn parse_signers_and_witnesses(
        server: &RpcServer,
        value: Option<&Value>,
    ) -> Result<(Option<Vec<Signer>>, Option<Vec<Witness>>), RpcException> {
        let Some(token_value) = value else {
            return Ok((None, None));
        };
        let jtoken: JToken = serde_json::from_value(token_value.clone())
            .map_err(|err| Self::invalid_params(err.to_string()))?;
        let ctx = ConversionContext::new(server.system().settings().address_version);
        let parsed = ParameterConverter::convert::<SignersAndWitnesses>(&jtoken, &ctx)?;
        let signers = if parsed.signers().is_empty() {
            None
        } else {
            Some(parsed.signers().to_vec())
        };
        let witnesses = if parsed.witnesses().is_empty() {
            None
        } else {
            Some(parsed.witnesses().to_vec())
        };
        Ok((signers, witnesses))
    }

    fn build_dynamic_call_script(
        script_hash: UInt160,
        operation: &str,
        parameters: &[ContractParameter],
    ) -> Result<Vec<u8>, RpcException> {
        let args = parameters
            .iter()
            .map(Self::contract_parameter_to_stack_item)
            .collect::<Result<Vec<_>, _>>()?;
        let mut builder = neo_vm::script_builder::ScriptBuilder::new();

        if args.is_empty() {
            builder.emit_opcode(neo_vm::op_code::OpCode::NEWARRAY0);
        } else {
            for item in args.iter().rev() {
                builder
                    .emit_push_stack_item(item.clone())
                    .map_err(|err| Self::internal_error(err.to_string()))?;
            }
            builder.emit_push_int(args.len() as i64);
            builder.emit_opcode(neo_vm::op_code::OpCode::PACK);
        }

        builder.emit_push_int(CallFlags::ALL.bits() as i64);
        builder.emit_push(operation.as_bytes());
        builder.emit_push(script_hash.to_bytes().as_ref());
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| Self::internal_error(err.to_string()))?;

        Ok(builder.to_array())
    }

    fn contract_parameter_to_stack_item(
        parameter: &ContractParameter,
    ) -> Result<StackItem, RpcException> {
        match &parameter.value {
            ContractParameterValue::Any | ContractParameterValue::Void => Ok(StackItem::Null),
            ContractParameterValue::Boolean(value) => Ok(StackItem::from_bool(*value)),
            ContractParameterValue::Integer(value) => Ok(StackItem::from_int(value.clone())),
            ContractParameterValue::Hash160(value) => {
                Ok(StackItem::from_byte_string(value.to_bytes().to_vec()))
            }
            ContractParameterValue::Hash256(value) => {
                Ok(StackItem::from_byte_string(value.to_array().to_vec()))
            }
            ContractParameterValue::ByteArray(bytes) | ContractParameterValue::Signature(bytes) => {
                Ok(StackItem::from_byte_string(bytes.clone()))
            }
            ContractParameterValue::PublicKey(point) => {
                Ok(StackItem::from_byte_string(point.encoded()))
            }
            ContractParameterValue::String(value) => {
                Ok(StackItem::from_byte_string(value.as_bytes().to_vec()))
            }
            ContractParameterValue::Array(items) => {
                let converted = items
                    .iter()
                    .map(Self::contract_parameter_to_stack_item)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(StackItem::from_array(converted))
            }
            ContractParameterValue::Map(entries) => {
                #[allow(clippy::mutable_key_type)]
                let mut map = BTreeMap::new();
                for (key, value) in entries {
                    let key_item = Self::contract_parameter_to_stack_item(key)?;
                    let value_item = Self::contract_parameter_to_stack_item(value)?;
                    map.insert(key_item, value_item);
                }
                Ok(StackItem::from_map(map))
            }
            ContractParameterValue::InteropInterface => Err(Self::invalid_params(
                "InteropInterface parameters are not supported in invoke RPCs",
            )),
        }
    }

    fn stack_item_to_json(
        item: &StackItem,
        mut session: Option<&mut Session>,
    ) -> Result<Value, RpcException> {
        let mut obj = Map::new();
        match item {
            StackItem::Null => {
                obj.insert("type".to_string(), Value::String("Any".to_string()));
                Ok(Value::Object(obj))
            }
            StackItem::Boolean(value) => {
                obj.insert("type".to_string(), Value::String("Boolean".to_string()));
                obj.insert("value".to_string(), Value::Bool(*value));
                Ok(Value::Object(obj))
            }
            StackItem::Integer(value) => {
                obj.insert("type".to_string(), Value::String("Integer".to_string()));
                obj.insert("value".to_string(), Value::String(value.to_string()));
                Ok(Value::Object(obj))
            }
            StackItem::ByteString(bytes) => {
                obj.insert("type".to_string(), Value::String("ByteString".to_string()));
                obj.insert(
                    "value".to_string(),
                    Value::String(BASE64_STANDARD.encode(bytes)),
                );
                Ok(Value::Object(obj))
            }
            StackItem::Buffer(buffer) => {
                obj.insert("type".to_string(), Value::String("Buffer".to_string()));
                obj.insert(
                    "value".to_string(),
                    Value::String(BASE64_STANDARD.encode(buffer.data())),
                );
                Ok(Value::Object(obj))
            }
            StackItem::Array(array) => {
                obj.insert("type".to_string(), Value::String("Array".to_string()));
                let values = array
                    .items()
                    .iter()
                    .map(|entry| Self::stack_item_to_json(entry, session.as_deref_mut()))
                    .collect::<Result<Vec<_>, _>>()?;
                obj.insert("value".to_string(), Value::Array(values));
                Ok(Value::Object(obj))
            }
            StackItem::Struct(items) => {
                obj.insert("type".to_string(), Value::String("Struct".to_string()));
                let values = items
                    .items()
                    .iter()
                    .map(|entry| Self::stack_item_to_json(entry, session.as_deref_mut()))
                    .collect::<Result<Vec<_>, _>>()?;
                obj.insert("value".to_string(), Value::Array(values));
                Ok(Value::Object(obj))
            }
            StackItem::Map(map) => {
                obj.insert("type".to_string(), Value::String("Map".to_string()));
                let entries = map
                    .iter()
                    .map(|(key, value)| {
                        let key_json = Self::stack_item_to_json(key, session.as_deref_mut())?;
                        let value_json = Self::stack_item_to_json(value, session.as_deref_mut())?;
                        Ok(json!({
                            "key": key_json,
                            "value": value_json,
                        }))
                    })
                    .collect::<Result<Vec<_>, RpcException>>()?;
                obj.insert("value".to_string(), Value::Array(entries));
                Ok(Value::Object(obj))
            }
            StackItem::Pointer(pointer) => {
                obj.insert("type".to_string(), Value::String("Pointer".to_string()));
                obj.insert(
                    "value".to_string(),
                    Value::Number(JsonNumber::from(pointer.position() as u64)),
                );
                Ok(Value::Object(obj))
            }
            StackItem::InteropInterface(iface) => {
                obj.insert(
                    "type".to_string(),
                    Value::String("InteropInterface".to_string()),
                );
                let mut value_obj = Map::new();
                value_obj.insert(
                    "type".to_string(),
                    Value::String(iface.interface_type().to_string()),
                );
                obj.insert("value".to_string(), Value::Object(value_obj));
                if let Some(session) = session.as_mut() {
                    if let Some(iterator_id) = session.register_iterator_interface(iface) {
                        obj.insert(
                            "interface".to_string(),
                            Value::String("IIterator".to_string()),
                        );
                        obj.insert("id".to_string(), Value::String(iterator_id.to_string()));
                    }
                }
                Ok(Value::Object(obj))
            }
        }
    }

    fn notification_to_json(
        notification: &NotifyEventArgs,
        mut session: Option<&mut Session>,
    ) -> Result<Value, RpcException> {
        let mut state = Vec::new();
        for entry in notification.state.iter() {
            state.push(Self::stack_item_to_json(entry, session.as_deref_mut())?);
        }
        Ok(json!({
            "eventname": notification.event_name,
            "contract": notification.script_hash.to_string(),
            "state": state,
        }))
    }

    fn diagnostic_invocation_to_json(diagnostic: &Diagnostic) -> Value {
        fn to_json_node(node_arc: Arc<Mutex<TreeNode<UInt160>>>) -> Value {
            if let Ok(node) = node_arc.lock() {
                let mut obj = Map::new();
                obj.insert("hash".to_string(), Value::String(node.item().to_string()));
                if !node.children().is_empty() {
                    let children = node
                        .children()
                        .iter()
                        .map(|child| to_json_node(child.clone()))
                        .collect::<Vec<_>>();
                    obj.insert("call".to_string(), Value::Array(children));
                }
                Value::Object(obj)
            } else {
                Value::Null
            }
        }

        match diagnostic.root() {
            Some(root) => to_json_node(root),
            None => Value::Null,
        }
    }

    fn diagnostic_storage_changes(engine: &ApplicationEngine) -> Value {
        let changes = engine.snapshot_cache().tracked_items();
        let entries = changes
            .into_iter()
            .map(|(key, trackable)| {
                json!({
                    "state": format!("{:?}", trackable.state),
                    "key": BASE64_STANDARD.encode(key.to_array()),
                    "value": BASE64_STANDARD.encode(trackable.item.get_value()),
                })
            })
            .collect::<Vec<_>>();
        Value::Array(entries)
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

    fn expect_u32_param(params: &[Value], index: usize, method: &str) -> Result<u32, RpcException> {
        let value = params.get(index).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data(format!(
                "{} expects integer parameter {}",
                method,
                index + 1
            )))
        })?;
        if let Some(number) = value.as_u64() {
            if number <= u32::MAX as u64 {
                return Ok(number as u32);
            }
        }
        Err(RpcException::from(RpcError::invalid_params().with_data(
            format!("{} expects integer parameter {}", method, index + 1),
        )))
    }

    fn expect_uuid_param(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<Uuid, RpcException> {
        let text = Self::expect_string_param(params, index, method)?;
        Uuid::parse_str(text.trim()).map_err(|_| {
            RpcException::from(RpcError::invalid_params().with_data(format!(
                "{} expects GUID parameter {}",
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
}
