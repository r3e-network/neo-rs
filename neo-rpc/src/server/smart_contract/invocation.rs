use std::str::FromStr;
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_core::UInt160;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::store_cache::StoreCache;
use neo_core::smart_contract::native::GasToken;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::wallets::{Wallet, WalletAccount};
use num_bigint::BigInt;
use rand::random;
use serde_json::{Map, Number as JsonNumber, Value, json};

use crate::server::diagnostic::Diagnostic;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use crate::server::session::Session;
use neo_vm::stack_item::StackItem;
use neo_vm::vm_state::VMState;

use super::helpers::{
    build_dynamic_call_script, diagnostic_invocation_to_json, diagnostic_storage_changes,
    expect_string_param, internal_error, invalid_params, notification_to_json,
    parse_contract_parameters, parse_signers_and_witnesses, stack_item_to_json,
};

const TRANSACTION_TYPE_NAME: &str = "Neo.Network.P2P.Payloads.Transaction";

enum WalletInvocationOutcome {
    Signed(Vec<u8>),
    Pending(Value),
}

#[derive(Clone)]
struct PendingSignatureItem {
    account: UInt160,
    script: Option<Vec<u8>>,
    parameter_types: Vec<neo_core::smart_contract::contract_parameter_type::ContractParameterType>,
}

pub(super) fn invoke_function(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
    let script_hash = expect_string_param(params, 0, "invokefunction")?;
    let script_hash = UInt160::from_str(&script_hash)
        .map_err(|err| invalid_params(format!("invalid script hash: {err}")))?;

    let operation = expect_string_param(params, 1, "invokefunction")?;
    let parameters = parse_contract_parameters(params.get(2))?;
    let (signers, witnesses) = parse_signers_and_witnesses(server, params.get(3))?;
    let use_diagnostic = params
        .get(4)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let script = build_dynamic_call_script(script_hash, &operation, &parameters)?;
    execute_script(server, script, signers, witnesses, use_diagnostic)
}

pub(super) fn invoke_script(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
    let script_base64 = expect_string_param(params, 0, "invokescript")?;
    let script = BASE64_STANDARD
        .decode(script_base64.trim())
        .map_err(|_| invalid_params("invalid script payload"))?;
    let (signers, witnesses) = parse_signers_and_witnesses(server, params.get(1))?;
    let use_diagnostic = params
        .get(2)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    execute_script(server, script, signers, witnesses, use_diagnostic)
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
    .map_err(internal_error)?;

    let (
        vm_state,
        engine_state,
        system_fee,
        exception_value,
        notifications_snapshot,
        stack_snapshot,
        diagnostics_snapshot,
    ) = {
        let engine = session.engine();
        let vm_state = engine.state();
        let engine_state = format!("{vm_state:?}");
        let system_fee = engine.fee_consumed();
        let exception_value = engine.fault_exception().map_or(Value::Null, |msg| {
            Value::String(normalize_fault_message(msg))
        });
        let notifications_snapshot = engine.notifications().to_vec();
        let stack_snapshot: Vec<StackItem> = engine.result_stack().iter().cloned().collect();
        let diagnostics_snapshot = session.diagnostic().map(|diag| {
            let invocation = diagnostic_invocation_to_json(&diag);
            let storage = diagnostic_storage_changes(&engine);
            (invocation, storage)
        });
        (
            vm_state,
            engine_state,
            system_fee,
            exception_value,
            notifications_snapshot,
            stack_snapshot,
            diagnostics_snapshot,
        )
    };
    let gas_consumed = system_fee.to_string();

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
        for notification in &notifications_snapshot {
            entries.push(notification_to_json(
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
        for item in &stack_snapshot {
            match stack_item_to_json(item, session_ref.as_deref_mut()) {
                Ok(value) => entries.push(value),
                Err(err) => entries.push(Value::String(format!("error: {err}"))),
            }
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

    if vm_state != VMState::FAULT {
        process_invoke_with_wallet(
            server,
            &mut result,
            session.script(),
            signers.as_ref(),
            session.snapshot(),
            system_fee,
        );
    }

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

    match build_and_sign_transaction(server, script, signers, snapshot, system_fee, wallet) {
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

fn normalize_fault_message(message: &str) -> String {
    if message.contains("Gas exhausted") || message.contains("Gas limit exceeded") {
        "Insufficient GAS".to_string()
    } else {
        message.to_string()
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

    let ledger = neo_core::smart_contract::native::ledger_contract::LedgerContract::new();
    let valid_until = ledger
        .current_index(snapshot)
        .map_err(|err| err.to_string())?
        .saturating_add(system.max_valid_until_block_increment());
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
    let pending = sign_transaction_with_wallet(&mut tx_clone, signers, &wallet);
    if pending.is_empty() {
        Ok(WalletInvocationOutcome::Signed(tx_clone.to_bytes()))
    } else {
        let context = build_pending_context(&tx_clone, pending, protocol_settings.network);
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
                Err(_) => pending.push(build_pending_item(signer.account, Some(account))),
            },
            Some(account) => pending.push(build_pending_item(signer.account, Some(account))),
            None => pending.push(build_pending_item(signer.account, None)),
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
            entry.script.map_or(Value::Null, |bytes| {
                Value::String(BASE64_STANDARD.encode(bytes))
            }),
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
