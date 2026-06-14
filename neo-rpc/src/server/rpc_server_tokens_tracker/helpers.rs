use crate::plugins::tokens_tracker::trackers::tracker_base::TokenTransferKeyView;
use crate::plugins::tokens_tracker::{
    Nep11TransferKey, Nep17TransferKey, TokenTransfer, TokensTrackerService, find_range,
};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, invalid_params};
use neo_execution::application_engine::TEST_MODE_GAS;
use neo_manifest::CallFlags;
use neo_primitives::UInt160;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use num_traits::ToPrimitive;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::RpcServer;

pub(super) fn tracker_service(
    server: &RpcServer,
) -> Result<Arc<TokensTrackerService>, RpcException> {
    server
        .system()
        .get_service::<TokensTrackerService>()
        .ok_or_else(|| RpcException::from(RpcError::method_not_found()))
}

pub(super) fn parse_address_param(
    params: &[Value],
    index: usize,
    method: &str,
    address_version: u8,
) -> Result<UInt160, RpcException> {
    let text = params
        .get(index)
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid_params(format!("{method} requires address parameter")))?;

    let mut parsed = None;
    if UInt160::try_parse(text, &mut parsed) {
        if let Some(value) = parsed {
            return Ok(value);
        }
    }

    neo_wallets::wallet_helper::WalletAddress::to_script_hash(text, address_version)
        .map_err(|_| invalid_params(format!("Invalid address: {text}")))
}

pub(super) fn parse_optional_u64(value: Option<&Value>) -> Result<u64, RpcException> {
    let Some(value) = value else {
        return Ok(0);
    };
    match value {
        Value::Null => Ok(0),
        Value::Number(num) => num
            .as_u64()
            .ok_or_else(|| invalid_params("Expected unsigned integer")),
        Value::String(text) => text
            .trim()
            .parse::<u64>()
            .map_err(|_| invalid_params("Expected unsigned integer")),
        _ => Err(invalid_params("Expected unsigned integer")),
    }
}

pub(super) fn parse_token_id_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Vec<u8>, RpcException> {
    let text = params
        .get(index)
        .and_then(|value| value.as_str())
        .ok_or_else(|| invalid_params(format!("{method} requires tokenId parameter")))?;
    hex::decode(text).map_err(|_| invalid_params("Invalid tokenId"))
}

pub(super) fn collect_transfers(
    store: &dyn neo_storage::persistence::Store,
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    address_version: u8,
    max_results: usize,
) -> Result<Value, RpcException> {
    let mut prefix_bytes = Vec::with_capacity(1 + UInt160::LENGTH);
    prefix_bytes.push(prefix);
    prefix_bytes.extend_from_slice(&script_hash.to_bytes());

    let start_key = [prefix_bytes.as_slice(), &start.to_be_bytes()].concat();
    let end_key = [prefix_bytes.as_slice(), &end.to_be_bytes()].concat();

    let pairs = find_range::<Nep17TransferKey, TokenTransfer>(store, &start_key, &end_key)
        .map_err(internal_error)?;

    let mut limited = pairs
        .into_iter()
        .take(max_results)
        .enumerate()
        .collect::<Vec<_>>();
    limited.sort_by(|(left_index, left), (right_index, right)| {
        let left_ts = left.0.timestamp_ms();
        let right_ts = right.0.timestamp_ms();
        right_ts
            .cmp(&left_ts)
            .then_with(|| left_index.cmp(right_index))
    });

    let mut entries = Vec::new();
    for (_, (key, value)) in limited {
        let transfer_address = if value.user_script_hash == UInt160::zero() {
            Value::Null
        } else {
            Value::String(neo_wallets::wallet_helper::WalletAddress::to_address(
                &value.user_script_hash,
                address_version,
            ))
        };
        entries.push(json!({
            "timestamp": key.timestamp_ms(),
            "assethash": key.asset_script_hash().to_string(),
            "transferaddress": transfer_address,
            "amount": value.amount.to_string(),
            "blockindex": value.block_index,
            "transfernotifyindex": key.block_xfer_notification_index(),
            "txhash": value.tx_hash.to_string()}));
    }

    Ok(Value::Array(entries))
}

pub(super) fn collect_nep11_transfers(
    store: &dyn neo_storage::persistence::Store,
    prefix: u8,
    script_hash: &UInt160,
    start: u64,
    end: u64,
    address_version: u8,
    max_results: usize,
) -> Result<Value, RpcException> {
    let mut prefix_bytes = Vec::with_capacity(1 + UInt160::LENGTH);
    prefix_bytes.push(prefix);
    prefix_bytes.extend_from_slice(&script_hash.to_bytes());

    let start_key = [prefix_bytes.as_slice(), &start.to_be_bytes()].concat();
    let end_key = [prefix_bytes.as_slice(), &end.to_be_bytes()].concat();

    let pairs = find_range::<Nep11TransferKey, TokenTransfer>(store, &start_key, &end_key)
        .map_err(internal_error)?;

    let mut limited = pairs
        .into_iter()
        .take(max_results)
        .enumerate()
        .collect::<Vec<_>>();
    limited.sort_by(|(left_index, left), (right_index, right)| {
        let left_ts = left.0.timestamp_ms();
        let right_ts = right.0.timestamp_ms();
        right_ts
            .cmp(&left_ts)
            .then_with(|| left_index.cmp(right_index))
    });

    let mut entries = Vec::new();
    for (_, (key, value)) in limited {
        let transfer_address = if value.user_script_hash == UInt160::zero() {
            Value::Null
        } else {
            Value::String(neo_wallets::wallet_helper::WalletAddress::to_address(
                &value.user_script_hash,
                address_version,
            ))
        };
        entries.push(json!({
            "timestamp": key.timestamp_ms(),
            "assethash": key.asset_script_hash().to_string(),
            "transferaddress": transfer_address,
            "amount": value.amount.to_string(),
            "blockindex": value.block_index,
            "transfernotifyindex": key.block_xfer_notification_index(),
            "txhash": value.tx_hash.to_string(),
            "tokenid": hex::encode(&key.token)}));
    }

    Ok(Value::Array(entries))
}

pub(super) fn query_asset_metadata(
    snapshot: &neo_storage::persistence::DataCache,
    settings: &neo_config::ProtocolSettings,
    asset: &UInt160,
) -> Option<(String, u32)> {
    let mut script = ScriptBuilder::new();
    emit_contract_call(&mut script, asset, "decimals").ok()?;
    emit_contract_call(&mut script, asset, "symbol").ok()?;

    let mut engine = neo_execution::ApplicationEngine::new(
        neo_primitives::TriggerType::Application,
        None,
        Arc::new(snapshot.clone()),
        None,
        settings.clone(),
        TEST_MODE_GAS,
        None,
    )
    .ok()?;
    engine
        .load_script(script.to_array(), CallFlags::ALL, Some(*asset))
        .ok()?;
    engine.execute().ok()?;
    if engine.state() != VMState::HALT {
        return None;
    }

    let result_stack = engine.result_stack();
    let symbol_item = result_stack.peek(0).ok()?;
    let decimals_item = result_stack.peek(1).ok()?;

    let symbol_bytes = symbol_item.as_bytes().ok()?;
    let symbol = String::from_utf8(symbol_bytes).ok()?;
    let decimals = decimals_item.as_integer().ok()?.to_u32()?;

    Some((symbol, decimals))
}

pub(super) fn emit_contract_call(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
) -> Result<(), RpcException> {
    builder.emit_opcode(OpCode::PUSH0);
    builder.emit_opcode(OpCode::NEWARRAY);
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push_string(method);
    builder.emit_push_byte_array(&contract.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| internal_error(err.to_string()))?;
    Ok(())
}

pub(super) fn emit_contract_call_with_arg(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
    call_flags: CallFlags,
    arg: &[u8],
) -> Result<(), RpcException> {
    builder.emit_push_byte_array(arg);
    builder.emit_push_int(1);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(i64::from(call_flags.bits()));
    builder.emit_push_string(method);
    builder.emit_push_byte_array(&contract.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| internal_error(err.to_string()))?;
    Ok(())
}

pub(super) fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
