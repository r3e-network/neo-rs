//! ApplicationEngine.Contract - ports Neo.SmartContract.ApplicationEngine.Contract.cs

use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::smart_contract::execution_context_state::ExecutionContextState;
use crate::smart_contract::iterators::IteratorInterop;
use crate::smart_contract::native::crypto_lib::Bls12381Interop;
use crate::UInt160;
use neo_vm::{ExecutionEngine, ExecutionEngineLimits, StackItem, VmError, VmResult};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

const SYSTEM_CONTRACT_CALL_PRICE: i64 = 1 << 15;

pub(crate) fn register_contract_interops(engine: &mut ApplicationEngine) -> VmResult<()> {
    engine.register_host_service(
        "System.Contract.Call",
        SYSTEM_CONTRACT_CALL_PRICE,
        CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
        contract_call_handler,
    )?;

    engine.register_host_service(
        "System.Contract.GetCallFlags",
        1 << 10,
        CallFlags::NONE,
        contract_get_call_flags_handler,
    )?;

    engine.register_host_service(
        "System.Contract.CreateStandardAccount",
        0,
        CallFlags::NONE,
        contract_create_standard_account_handler,
    )?;

    engine.register_host_service(
        "System.Contract.CreateMultisigAccount",
        0,
        CallFlags::NONE,
        contract_create_multisig_account_handler,
    )?;

    engine.register_host_service(
        "System.Contract.CallNative",
        0,
        CallFlags::NONE,
        contract_call_native_handler,
    )?;

    engine.register_host_service(
        "System.Contract.NativeOnPersist",
        0,
        CallFlags::STATES,
        contract_native_on_persist_handler,
    )?;

    engine.register_host_service(
        "System.Contract.NativePostPersist",
        0,
        CallFlags::STATES,
        contract_native_post_persist_handler,
    )?;

    Ok(())
}

fn map_contract_result(service: &str, result: Result<(), String>) -> VmResult<()> {
    result.map_err(|error| VmError::InteropService {
        service: service.to_string(),
        error,
    })
}

fn contract_call_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // C# parity (Neo.Extensions.ScriptBuilderExtensions.EmitDynamicCall):
    // stack before SYSCALL: [args_array, call_flags, method, contract_hash]
    // parameters are consumed in declaration order:
    //   CallContract(UInt160 contractHash, string method, CallFlags callFlags, Array args)
    // so we must pop: hash -> method -> flags -> args.
    let hash_bytes = app.pop_bytes().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e,
    })?;

    let method = app.pop_string().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e,
    })?;

    let call_flags_value = app.pop_integer().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e,
    })?;

    let args = app.pop_array().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e,
    })?;

    let result = (|| {
        if hash_bytes.len() != 20 {
            return Err("Contract hash must be 20 bytes".to_string());
        }

        let contract_hash = UInt160::from_bytes(&hash_bytes)
            .map_err(|e| format!("Invalid contract hash: {}", e))?;

        if call_flags_value < 0 || call_flags_value > u8::MAX as i64 {
            return Err("Invalid call flags value".to_string());
        }

        let call_flags = CallFlags::from_bits(call_flags_value as u8)
            .ok_or_else(|| "Call flags contain unsupported bits".to_string())?;

        app.call_contract_dynamic(&contract_hash, &method, call_flags, args)
            .map_err(|e| e.to_string())
    })();

    map_contract_result("System.Contract.Call", result)
}

fn contract_get_call_flags_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = (|| {
        let flags = app.get_current_call_flags().map_err(|e| e.to_string())?;
        app.push_integer(i64::from(flags.bits()))?;
        Ok(())
    })();

    map_contract_result("System.Contract.GetCallFlags", result)
}

fn contract_create_standard_account_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let pub_key_bytes = app.pop_bytes().map_err(|e| VmError::InteropService {
        service: "System.Contract.CreateStandardAccount".to_string(),
        error: e,
    })?;

    let result = match app.create_standard_account(&pub_key_bytes) {
        Ok(account) => app
            .push_bytes(account.to_bytes())
            .map_err(|e| VmError::InteropService {
                service: "System.Contract.CreateStandardAccount".to_string(),
                error: e,
            })
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Err(err) => Err(err.to_string()),
    };

    map_contract_result("System.Contract.CreateStandardAccount", result)
}

fn contract_create_multisig_account_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let public_keys_items = app.pop_array().map_err(|e| VmError::InteropService {
        service: "System.Contract.CreateMultisigAccount".to_string(),
        error: e,
    })?;

    let m = app.pop_integer().map_err(|e| VmError::InteropService {
        service: "System.Contract.CreateMultisigAccount".to_string(),
        error: e,
    })?;

    let result = (|| {
        if m < 0 || m > i32::MAX as i64 {
            return Err("Invalid multisig threshold".to_string());
        }

        let account = app
            .create_multisig_account(m as i32, public_keys_items)
            .map_err(|e| e.to_string())?;
        app.push_bytes(account.to_bytes())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();

    map_contract_result("System.Contract.CreateMultisigAccount", result)
}

fn contract_call_native_handler(
    app: &mut ApplicationEngine,
    engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = (|| -> Result<(), String> {
        let version_item = engine.pop().map_err(|e| e.to_string())?;
        let version_big = version_item.as_int().map_err(|e| e.to_string())?;
        if !version_big.is_zero() {
            return Err(format!(
                "Unsupported native contract version {}",
                version_big
            ));
        }

        let context = engine
            .current_context()
            .cloned()
            .ok_or_else(|| "No current execution context".to_string())?;
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let (script_hash, method_name, arg_count, return_type, parameter_types) = {
            let state = state_arc.lock();
            let script_hash = state
                .script_hash
                .ok_or_else(|| "Native contract context missing script hash".to_string())?;
            let method_name = state
                .method_name
                .clone()
                .ok_or_else(|| "Native contract context missing method name".to_string())?;
            let arg_count = state.argument_count;
            let return_type = state.return_type;
            let parameter_types = state.parameter_types.clone();
            (
                script_hash,
                method_name,
                arg_count,
                return_type,
                parameter_types,
            )
        };

        let stack_len = context.evaluation_stack().len();
        if arg_count > stack_len {
            return Err(format!(
                "Native contract expected {} argument(s) but stack contains {}",
                arg_count, stack_len
            ));
        }

        let mut args = Vec::with_capacity(arg_count);
        for index in 0..arg_count {
            let item = engine.pop().map_err(|e| e.to_string())?;
            let bytes = match parameter_types.get(index) {
                Some(ContractParameterType::Any) => {
                    BinarySerializer::serialize(&item, app.execution_limits())
                        .map_err(|e| e.to_string())?
                }
                Some(ContractParameterType::InteropInterface) => stack_item_to_interop_bytes(item)?,
                _ => ApplicationEngine::stack_item_to_bytes(item)?,
            };
            args.push(bytes);
        }

        let result_bytes = app
            .call_native_contract(script_hash, &method_name, &args)
            .map_err(|e| e.to_string())?;

        {
            let mut state = state_arc.lock();
            state.argument_count = 0;
            state.method_name = None;
            state.return_type = None;
            state.parameter_types.clear();
        }

        if let Some(ret_type) = return_type {
            push_native_result(engine, ret_type, result_bytes)?;
        }

        // Load any queued calls requested by the native method (e.g. NEP-17 callbacks).
        app.process_pending_native_calls()
            .map_err(|e| e.to_string())?;

        Ok(())
    })();

    map_contract_result("System.Contract.CallNative", result)
}

fn push_native_result(
    engine: &mut ExecutionEngine,
    return_type: ContractParameterType,
    result: Vec<u8>,
) -> Result<(), String> {
    match return_type {
        ContractParameterType::Void => Ok(()),
        ContractParameterType::Boolean => {
            let value = result.iter().any(|byte| *byte != 0);
            engine
                .push(StackItem::from_bool(value))
                .map_err(|e| e.to_string())
        }
        ContractParameterType::Integer => {
            let big = BigInt::from_signed_bytes_le(&result);
            engine
                .push(StackItem::from_int(big))
                .map_err(|e| e.to_string())
        }
        ContractParameterType::String => {
            let string_bytes = String::from_utf8(result.clone())
                .map_err(|_| "Invalid UTF-8 string returned by native contract".to_string())?
                .into_bytes();
            engine
                .push(StackItem::from_byte_string(string_bytes))
                .map_err(|e| e.to_string())
        }
        ContractParameterType::Array | ContractParameterType::Map | ContractParameterType::Any => {
            match BinarySerializer::deserialize(&result, &ExecutionEngineLimits::default(), None) {
                Ok(item) => engine.push(item).map_err(|e| e.to_string()),
                Err(_) => engine
                    .push(StackItem::from_byte_string(result))
                    .map_err(|e| e.to_string()),
            }
        }
        ContractParameterType::InteropInterface => {
            if result.len() == 4 {
                let id = BigInt::from_signed_bytes_le(&result);
                let iterator_id = id
                    .to_u32()
                    .ok_or_else(|| "Iterator identifier out of range".to_string())?;
                return engine
                    .push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
                    .map_err(|e| e.to_string());
            }

            let interop =
                Bls12381Interop::from_encoded_bytes(&result).map_err(|e| e.to_string())?;
            engine
                .push(StackItem::from_interface(interop))
                .map_err(|e| e.to_string())
        }
        _ => engine
            .push(StackItem::from_byte_string(result))
            .map_err(|e| e.to_string()),
    }
}

fn stack_item_to_interop_bytes(item: StackItem) -> Result<Vec<u8>, String> {
    match item {
        StackItem::InteropInterface(interface) => interface
            .as_any()
            .downcast_ref::<Bls12381Interop>()
            .map(|interop| interop.to_encoded_bytes())
            .ok_or_else(|| "Invalid interop interface argument".to_string()),
        _ => Err("Stack item is not an InteropInterface".to_string()),
    }
}

fn contract_native_on_persist_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = app.native_on_persist().map_err(|e| e.to_string());
    map_contract_result("System.Contract.NativeOnPersist", result)
}

fn contract_native_post_persist_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = app.native_post_persist().map_err(|e| e.to_string());
    map_contract_result("System.Contract.NativePostPersist", result)
}
