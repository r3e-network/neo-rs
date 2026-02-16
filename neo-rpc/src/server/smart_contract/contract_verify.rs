use std::str::FromStr;
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_core::UInt160;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter_type::ContractParameterType;
use neo_core::smart_contract::{ApplicationEngine, TriggerType};
use rand::random;
use serde_json::{Value, json};

use crate::server::rpc_error::RpcError;
use crate::server::rpc_error_factory;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use neo_vm::op_code::OpCode;

use super::helpers::{
    internal_error, invalid_params, parse_contract_parameters, parse_signers_and_witnesses,
    stack_item_to_json_limited,
};

pub(super) fn invoke_contract_verify(
    server: &RpcServer,
    params: &[Value],
) -> Result<Value, RpcException> {
    let script_hash = super::helpers::expect_string_param(params, 0, "invokecontractverify")?;
    let script_hash = UInt160::from_str(&script_hash)
        .map_err(|err| invalid_params(format!("invalid script hash: {err}")))?;

    let parameters = parse_contract_parameters(params.get(1))?;
    let (signers, witnesses) = parse_signers_and_witnesses(server, params.get(2))?;

    let system = server.system();
    let store_cache = system.store_cache();
    let snapshot_cache = Arc::new(store_cache.data_cache().clone());

    let contract =
        neo_core::smart_contract::native::contract_management::ContractManagement::get_contract_from_snapshot(
            snapshot_cache.as_ref(),
            &script_hash,
        )
        .map_err(|err| internal_error(err.to_string()))?
        .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;

    let verify_method = contract
        .manifest
        .abi
        .get_method_ref("verify", parameters.len())
        .cloned()
        .ok_or_else(|| {
            RpcException::from(rpc_error_factory::invalid_contract_verification_hash(
                &contract.hash,
                parameters.len() as i32,
            ))
        })?;

    if verify_method.return_type != ContractParameterType::Boolean {
        return Err(RpcException::from(
            rpc_error_factory::invalid_contract_verification(
                "The verify method doesn't return boolean value.",
            ),
        ));
    }

    let signers =
        signers.unwrap_or_else(|| vec![Signer::new(script_hash, neo_core::WitnessScope::NONE)]);
    let mut witnesses = witnesses.unwrap_or_default();

    let mut invocation_script = Vec::new();
    if !parameters.is_empty() {
        invocation_script = build_verification_invocation_script(&parameters)?;
        if witnesses.is_empty() {
            witnesses.push(Witness::new_with_scripts(
                invocation_script.clone(),
                Vec::new(),
            ));
        }
    }

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(random());
    tx.set_signers(signers);
    tx.set_attributes(Vec::<TransactionAttribute>::new());
    tx.set_witnesses(witnesses);
    tx.set_script(vec![OpCode::RET as u8]);

    let tx_container = Arc::new(tx) as Arc<dyn neo_core::IVerifiable>;
    let mut engine = ApplicationEngine::new(
        TriggerType::Verification,
        Some(tx_container),
        Arc::clone(&snapshot_cache),
        None,
        system.settings().clone(),
        server.settings().max_gas_invoke,
        None,
    )
    .map_err(|err| internal_error(err.to_string()))?;

    engine
        .load_contract_method(contract, verify_method, CallFlags::READ_ONLY)
        .map_err(|err| internal_error(err.to_string()))?;
    if !invocation_script.is_empty() {
        engine
            .load_script(invocation_script.clone(), CallFlags::NONE, None)
            .map_err(|err| internal_error(err.to_string()))?;
    }

    engine.execute_allow_fault();

    let state = engine.state();
    let mut exception = engine
        .fault_exception()
        .map_or(Value::Null, |message| Value::String(message.to_string()));

    let mut stack_items = Vec::new();
    let mut stack_error: Option<RpcException> = None;
    for item in engine.result_stack().iter() {
        match stack_item_to_json_limited(item, None, server.settings().max_stack_size) {
            Ok(value) => stack_items.push(value),
            Err(err) => {
                stack_error = Some(err);
                break;
            }
        }
    }
    if let Some(err) = stack_error.as_ref() {
        exception = Value::String(err.to_string());
    }

    let mut result = json!({
        "script": BASE64_STANDARD.encode(&invocation_script),
        "state": format!("{:?}", state),
        "gasconsumed": engine.fee_consumed().to_string(),
        "exception": exception,
    });
    if stack_error.is_none() {
        if let Value::Object(ref mut obj) = result {
            obj.insert("stack".to_string(), Value::Array(stack_items));
        }
    }

    Ok(result)
}

fn build_verification_invocation_script(
    parameters: &[neo_core::smart_contract::contract_parameter::ContractParameter],
) -> Result<Vec<u8>, RpcException> {
    let mut builder = neo_vm::script_builder::ScriptBuilder::new();
    for parameter in parameters.iter().rev() {
        let item = super::helpers::contract_parameter_to_stack_item(parameter)?;
        builder
            .emit_push_stack_item(item)
            .map_err(|err| internal_error(err.to_string()))?;
    }
    Ok(builder.to_array())
}
