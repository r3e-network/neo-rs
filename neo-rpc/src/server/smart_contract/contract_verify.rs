use std::sync::Arc;

use neo_execution::{ApplicationEngine, TriggerType};
use neo_manifest::CallFlags;
use neo_payloads::VerifiableContainer;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_payloads::witness::Witness;
use neo_primitives::ContractParameterType;
use rand::random;
use serde_json::Value;

use crate::server::contract_state_provider::{
    DeployedContractProvider, DeployedContractProviderFactory,
    NativeDeployedContractProviderFactory,
};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_error_factory;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use neo_vm::OpCode;

use super::helpers::internal_error;
use super::invoke::contract_parameter_to_stack_value;
use super::request::InvokeContractVerifyRequest;
use super::response::{
    final_rpc_vm_state_string, insert_stack, invoke_result_base_to_json, stack_item_to_json_limited,
};

pub(super) fn invoke_contract_verify(
    server: &RpcServer,
    params: &[Value],
) -> Result<Value, RpcException> {
    let request = InvokeContractVerifyRequest::parse(server, params)?;

    let system = server.system();
    let store_cache = system.store_cache();
    let snapshot_cache = Arc::new(store_cache.data_cache().clone());

    let contract = NativeDeployedContractProviderFactory
        .provider()
        .contract_state_by_hash(snapshot_cache.as_ref(), &request.script_hash)
        .map_err(|err| internal_error(err.to_string()))?
        .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;

    let verify_method = contract
        .manifest
        .abi
        .get_method_ref("verify", request.parameters.len())
        .cloned()
        .ok_or_else(|| {
            RpcException::from(RpcError::invalid_contract_verification_hash(
                &contract.hash,
                request.parameters.len() as i32,
            ))
        })?;

    if verify_method.return_type != ContractParameterType::Boolean {
        return Err(RpcException::from(
            rpc_error_factory::invalid_contract_verification(
                "The verify method doesn't return boolean value.",
            ),
        ));
    }

    let signers = request.signers.unwrap_or_else(|| {
        vec![Signer::new(
            request.script_hash,
            neo_primitives::WitnessScope::NONE,
        )]
    });
    let mut witnesses = request.witnesses.unwrap_or_default();

    let mut invocation_script = Vec::new();
    if !request.parameters.is_empty() {
        invocation_script = build_verification_invocation_script(&request.parameters)?;
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
    tx.set_script(vec![OpCode::RET.byte()]);

    let tx_container = Arc::new(VerifiableContainer::from(tx));
    let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::Verification,
        Some(tx_container),
        Arc::clone(&snapshot_cache),
        None,
        system.settings().as_ref().clone(),
        server.settings().max_gas_invoke,
        neo_execution::NoDiagnostic,
        system.native_contract_provider(),
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

    let state = final_rpc_vm_state_string(engine.state())?;
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

    let mut result =
        invoke_result_base_to_json(&invocation_script, state, engine.fee_consumed(), exception);
    if stack_error.is_none() {
        insert_stack(&mut result, stack_items);
    }

    Ok(Value::Object(result))
}

fn build_verification_invocation_script(
    parameters: &[neo_execution::contract_parameter::ContractParameter],
) -> Result<Vec<u8>, RpcException> {
    let mut builder = neo_vm::script_builder::ScriptBuilder::new();
    for parameter in parameters.iter().rev() {
        let item = contract_parameter_to_stack_value(parameter)?;
        builder
            .emit_push_stack_value(&item)
            .map_err(|err| internal_error(err.to_string()))?;
    }
    Ok(builder.to_array())
}
