//! ApplicationEngine.Contract - ports Neo.SmartContract.ApplicationEngine.Contract.cs

use crate::ApplicationExecutionEngine as ExecutionEngine;
use crate::application_engine::ApplicationEngine;
use crate::bls12381_interop::Bls12381Interop;
use crate::env_flags::env_flag_enabled;
use crate::iterators::IteratorInterop;
use crate::native_contract::NativeContract;
use crate::native_contract_provider::NativeContractProvider;
use neo_crypto::bls12381_point::{G1_COMPRESSED_SIZE, G2_COMPRESSED_SIZE, GT_SIZE};
use neo_error::{CoreError, CoreResult};
use neo_payloads::Transaction;
use neo_primitives::CallFlags;
use neo_primitives::ContractParameterType;
use neo_primitives::hex_util;
use neo_primitives::{UInt160, UInt256};
use neo_serialization::BinarySerializer;
use neo_vm::ExecutionEngineLimits;
use neo_vm::{StackItem, VmError, VmResult};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::str::FromStr;
use std::sync::OnceLock;

const SYSTEM_CONTRACT_CALL_PRICE: i64 = 1 << 15;

fn native_call_trace_filter_matches(
    tx_filter: Option<UInt256>,
    trace_all: bool,
    tx_hash: Option<UInt256>,
) -> bool {
    contract_call_trace_filter_matches(tx_filter, trace_all, tx_hash)
}

fn native_call_trace_tx_filter() -> Option<UInt256> {
    std::env::var("NEO_TRACE_CALL_NATIVE_TX")
        .ok()
        .and_then(|raw| UInt256::from_str(raw.trim()).ok())
}

fn native_call_trace_enabled<P, D, B>(app: &ApplicationEngine<P, D, B>) -> bool
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    static TRACE_ALL: OnceLock<bool> = OnceLock::new();
    native_call_trace_filter_matches(
        native_call_trace_tx_filter(),
        *TRACE_ALL.get_or_init(|| env_flag_enabled("NEO_TRACE_CALL_NATIVE", false)),
        current_transaction_hash(app),
    )
}

fn contract_call_trace_filter_matches(
    tx_filter: Option<UInt256>,
    trace_all: bool,
    tx_hash: Option<UInt256>,
) -> bool {
    trace_all || matches!((tx_filter, tx_hash), (Some(filter), Some(hash)) if filter == hash)
}

fn contract_call_trace_tx_filter() -> Option<UInt256> {
    std::env::var("NEO_TRACE_CONTRACT_CALL_TX")
        .ok()
        .and_then(|raw| UInt256::from_str(raw.trim()).ok())
}

fn current_transaction_hash<P, D, B>(app: &ApplicationEngine<P, D, B>) -> Option<UInt256>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    app.script_container()
        .and_then(|container| container.as_transaction())
        .map(Transaction::hash)
}

fn contract_call_trace_enabled<P, D, B>(app: &ApplicationEngine<P, D, B>) -> bool
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    contract_call_trace_filter_matches(
        contract_call_trace_tx_filter(),
        env_flag_enabled("NEO_TRACE_CONTRACT_CALL", false),
        current_transaction_hash(app),
    )
}

fn trace_hex_prefix(bytes: &[u8]) -> String {
    let prefix_len = bytes.len().min(32);
    let mut text = hex_util::encode_hex(&bytes[..prefix_len]);
    if bytes.len() > prefix_len {
        text.push_str("...");
    }
    text
}

fn trace_stack_item_summary(item: &StackItem) -> String {
    match item {
        StackItem::Null => "Null".to_string(),
        StackItem::Boolean(value) => format!("Boolean({value})"),
        StackItem::Integer(value) => format!("Integer({})", value.to_bigint()),
        StackItem::ByteString(bytes) => {
            format!(
                "ByteString(len={},hex={})",
                bytes.len(),
                trace_hex_prefix(bytes)
            )
        }
        StackItem::Buffer(buffer) => buffer.with_data(|bytes| {
            format!(
                "Buffer(len={},hex={})",
                bytes.len(),
                trace_hex_prefix(bytes)
            )
        }),
        StackItem::Array(items) => format!("Array(len={})", items.len()),
        StackItem::Struct(items) => format!("Struct(len={})", items.len()),
        StackItem::Map(items) => format!("Map(len={})", items.len()),
        StackItem::Pointer(_) => "Pointer".to_string(),
        StackItem::InteropInterface(_) => "InteropInterface".to_string(),
    }
}

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    pub(crate) fn register_contract_interops(&mut self) -> VmResult<()> {
        self.register_host_service(
            "System.Contract.Call",
            SYSTEM_CONTRACT_CALL_PRICE,
            CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
            contract_call_handler,
        )?;

        self.register_host_service(
            "System.Contract.GetCallFlags",
            1 << 10,
            CallFlags::NONE,
            contract_get_call_flags_handler,
        )?;

        self.register_host_service(
            "System.Contract.CreateStandardAccount",
            0,
            CallFlags::NONE,
            contract_create_standard_account_handler,
        )?;

        self.register_host_service(
            "System.Contract.CreateMultisigAccount",
            0,
            CallFlags::NONE,
            contract_create_multisig_account_handler,
        )?;

        self.register_host_service(
            "System.Contract.CallNative",
            0,
            CallFlags::NONE,
            contract_call_native_handler,
        )?;

        self.register_host_service(
            "System.Contract.NativeOnPersist",
            0,
            CallFlags::STATES,
            contract_native_on_persist_handler,
        )?;

        self.register_host_service(
            "System.Contract.NativePostPersist",
            0,
            CallFlags::STATES,
            contract_native_post_persist_handler,
        )?;

        Ok(())
    }
}

fn map_contract_result(service: &str, result: CoreResult<()>) -> VmResult<()> {
    result.map_err(|error| VmError::InteropService {
        service: service.to_string(),
        error: error.to_string(),
    })
}

fn contract_call_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    // C# parity (Neo.Extensions.ScriptBuilderExtensions.EmitDynamicCall):
    // stack before SYSCALL: [args_array, call_flags, method, contract_hash]
    // parameters are consumed in declaration order:
    //   CallContract(UInt160 contractHash, string method, CallFlags callFlags, Array args)
    // so we must pop: hash -> method -> flags -> args.
    let hash_bytes = app.pop_bytes().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e.to_string(),
    })?;

    let method = app.pop_string().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e.to_string(),
    })?;

    let call_flags_value = app.pop_integer().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e.to_string(),
    })?;

    let args = app.pop_array().map_err(|e| VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error: e.to_string(),
    })?;

    if contract_call_trace_enabled(app) {
        let tx_hash = current_transaction_hash(app)
            .map(|hash| hash.to_string())
            .unwrap_or_else(|| "none".to_string());
        let args_summary = args
            .iter()
            .enumerate()
            .map(|(index, item)| format!("#{index}:{}", trace_stack_item_summary(item)))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "trace contract.call: tx={} target_raw={} method={} flags={} args=[{}]",
            tx_hash,
            hex_util::encode_hex(&hash_bytes),
            method,
            call_flags_value,
            args_summary
        );
    }

    let result = (|| -> CoreResult<()> {
        if hash_bytes.len() != 20 {
            return Err(CoreError::other("Contract hash must be 20 bytes"));
        }

        let contract_hash = UInt160::from_bytes(&hash_bytes)
            .map_err(|e| CoreError::other(format!("Invalid contract hash: {}", e)))?;

        if call_flags_value < 0 || call_flags_value > u8::MAX as i64 {
            return Err(CoreError::other("Invalid call flags value"));
        }

        let call_flags = CallFlags::from_bits(call_flags_value as u8)
            .ok_or_else(|| CoreError::other("Call flags contain unsupported bits"))?;

        app.call_contract_dynamic(&contract_hash, &method, call_flags, args)
    })();

    map_contract_result("System.Contract.Call", result)
}

fn contract_get_call_flags_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let result = (|| -> CoreResult<()> {
        let flags = app.get_current_call_flags()?;
        app.push_integer(i64::from(flags.bits()))?;
        Ok(())
    })();

    map_contract_result("System.Contract.GetCallFlags", result)
}

fn contract_create_standard_account_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let pub_key_bytes = app.pop_bytes().map_err(|e| VmError::InteropService {
        service: "System.Contract.CreateStandardAccount".to_string(),
        error: e.to_string(),
    })?;

    let result = match app.create_standard_account(&pub_key_bytes) {
        Ok(account) => app
            .push_bytes(account.to_bytes())
            .map_err(|e| VmError::InteropService {
                service: "System.Contract.CreateStandardAccount".to_string(),
                error: e.to_string(),
            })
            .map(|_| ())
            .map_err(|e: VmError| CoreError::other(e.to_string())),
        Err(err) => Err(CoreError::other(err.to_string())),
    };

    map_contract_result("System.Contract.CreateStandardAccount", result)
}

fn contract_create_multisig_account_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    // C# parity (Neo.SmartContract.ApplicationEngine.Contract.cs):
    //   CreateMultisigAccount(int m, ECPoint[] pubKeys)
    // Parameters are consumed in declaration order: pop m first (top of stack),
    // then pubKeys. Caller's SWAP between LDLOC0/LDARG0 confirms m-on-top layout.
    let m = app.pop_integer().map_err(|e| VmError::InteropService {
        service: "System.Contract.CreateMultisigAccount".to_string(),
        error: e.to_string(),
    })?;

    let public_keys_items = app.pop_array().map_err(|e| VmError::InteropService {
        service: "System.Contract.CreateMultisigAccount".to_string(),
        error: e.to_string(),
    })?;

    let result = (|| -> CoreResult<()> {
        if m < i32::MIN as i64 || m > i32::MAX as i64 {
            return Err(CoreError::other("Invalid multisig threshold"));
        }

        let account = app.create_multisig_account(m as i32, public_keys_items)?;
        app.push_bytes(account.to_bytes())?;
        Ok(())
    })();

    map_contract_result("System.Contract.CreateMultisigAccount", result)
}

fn contract_call_native_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let result = (|| -> CoreResult<()> {
        let version_item = engine.pop().map_err(|e| CoreError::other(e.to_string()))?;
        let version_big = version_item
            .as_int()
            .map_err(|e| CoreError::other(e.to_string()))?;
        if !version_big.is_zero() {
            return Err(CoreError::other(format!(
                "Unsupported native contract version {}",
                version_big
            )));
        }

        let (state_arc, stack_len) = {
            let context = engine
                .current_context()
                .ok_or_else(|| CoreError::other("No current execution context"))?;
            let state_arc = context.state();
            let stack_len = context.evaluation_stack().len();
            (state_arc, stack_len)
        };

        let (script_hash, method_name, arg_count, return_type, parameter_types) = {
            let state = state_arc.lock();
            let script_hash = state
                .script_hash
                .ok_or_else(|| CoreError::other("Native contract context missing script hash"))?;
            let method_name = state
                .method_name
                .clone()
                .ok_or_else(|| CoreError::other("Native contract context missing method name"))?;
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

        let trace_native_call = native_call_trace_enabled(app);
        if trace_native_call {
            let tx_hash = current_transaction_hash(app)
                .map(|hash| hash.to_string())
                .unwrap_or_else(|| "none".to_string());
            let caller_hash = app.get_calling_script_hash().unwrap_or_else(UInt160::zero);
            let current_hash = app.current_script_hash().unwrap_or_else(UInt160::zero);
            eprintln!(
                "call_native begin tx={} contract={} method={} arg_count={} param_types={:?} current={} caller={}",
                tx_hash,
                script_hash,
                method_name,
                arg_count,
                parameter_types,
                current_hash,
                caller_hash
            );
        }

        if arg_count > stack_len {
            return Err(CoreError::other(format!(
                "Native contract expected {} argument(s) but stack contains {}",
                arg_count, stack_len
            )));
        }

        let mut args = Vec::with_capacity(arg_count);
        let mut null_mask: u32 = 0;
        for index in 0..arg_count {
            let item = engine.pop().map_err(|e| CoreError::other(e.to_string()))?;
            if matches!(item, StackItem::Null) && index < 32 {
                null_mask |= 1u32 << index;
            }
            if trace_native_call {
                let stack_type = item.stack_item_type();
                let bytes_len = item.as_bytes().map(|value| value.len());
                eprintln!(
                    "call_native stack_item[{index}] vm_type={stack_type:?} as_bytes_len={bytes_len:?}"
                );
            }
            args.push(item);
        }

        app.begin_native_call(null_mask);
        let call_result = call_native_contract_stack_items(
            app,
            script_hash,
            &method_name,
            args,
            trace_native_call,
        );
        let force_null_return = app.finish_native_call();
        let result_item = call_result?;

        if trace_native_call {
            let tx_hash = current_transaction_hash(app)
                .map(|hash| hash.to_string())
                .unwrap_or_else(|| "none".to_string());
            eprintln!(
                "call_native result tx={} contract={} method={} item={}",
                tx_hash,
                script_hash,
                method_name,
                result_item
                    .as_ref()
                    .map(trace_stack_item_summary)
                    .unwrap_or_else(|| "Void".to_string())
            );
        }

        {
            let mut state = state_arc.lock();
            state.argument_count = 0;
            state.method_name = None;
            state.return_type = None;
            state.parameter_types.clear();
        }

        if let Some(ret_type) = return_type {
            if force_null_return {
                // The native method explicitly returned `null` (e.g. a failed
                // `recoverSecp256K1`); push `Null` rather than the empty payload.
                engine
                    .push(StackItem::null())
                    .map_err(|e| CoreError::other(e.to_string()))?;
            } else if let Some(item) = result_item {
                engine
                    .push(item)
                    .map_err(|e| CoreError::other(e.to_string()))?;
            } else if ret_type != ContractParameterType::Void {
                return Err(CoreError::other(format!(
                    "Native contract method {method_name} did not return a value"
                )));
            }
        }

        // Load any queued calls requested by the native method (e.g. NEP-17 callbacks).
        app.process_pending_native_calls()?;

        Ok(())
    })();

    map_contract_result("System.Contract.CallNative", result)
}

fn call_native_contract_stack_items<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    contract_hash: UInt160,
    method_name: &str,
    args: Vec<StackItem>,
    trace_native_call: bool,
) -> CoreResult<Option<StackItem>>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    app.with_resolved_native_method(
        contract_hash,
        method_name,
        args.len(),
        move |native, resolved_method, engine| {
            let access = engine
                .execution_observations_enabled()
                .then(|| engine.native_call_access(native, resolved_method, args.len()))
                .flatten();
            let observed_arguments = access.as_ref().map(|_| args.clone());
            let result = if let Some(result) = native.try_invoke_resolved_stack_items(
                engine,
                resolved_method.method_index(),
                resolved_method.method(),
                &args,
            ) {
                result
            } else {
                (|| {
                    let mut encoded_args = Vec::with_capacity(args.len());
                    for (index, item) in args.into_iter().enumerate() {
                        let parameter_type = resolved_method.method().parameters.get(index);
                        let bytes = match parameter_type {
                            Some(ContractParameterType::Any) => {
                                BinarySerializer::serialize(&item, engine.execution_limits())?
                            }
                            Some(ContractParameterType::InteropInterface) => {
                                stack_item_to_interop_bytes(item)?
                            }
                            _ => ApplicationEngine::<P, D>::stack_item_to_bytes(item)?,
                        };
                        if trace_native_call {
                            let preview_len = bytes.len().min(24);
                            eprintln!(
                                "call_native arg[{index}] type={parameter_type:?} len={} preview=0x{}",
                                bytes.len(),
                                hex_util::encode_hex(&bytes[..preview_len])
                            );
                        }
                        encoded_args.push(bytes);
                    }

                    let result = native.invoke_resolved(
                        engine,
                        resolved_method.method_index(),
                        resolved_method.method(),
                        &encoded_args,
                    )?;
                    decode_native_result(resolved_method.method().return_type, result)
                })()
            };
            if let (Some(access), Some(observed_arguments)) = (access, observed_arguments) {
                let outcome = match &result {
                    Ok(_) if access.result_count() == 0 => {
                        crate::execution_artifact::CallObservationOutcome::Returned(Vec::new())
                    }
                    Ok(_) if engine.native_return_is_null() => {
                        crate::execution_artifact::CallObservationOutcome::Returned(vec![
                            StackItem::null(),
                        ])
                    }
                    Ok(value) => crate::execution_artifact::CallObservationOutcome::Returned(
                        value.iter().cloned().collect(),
                    ),
                    Err(error) => crate::execution_artifact::CallObservationOutcome::Fault {
                        message: error.to_string(),
                        exception: None,
                    },
                };
                engine.observe_completed_call(access, observed_arguments, outcome);
            }
            result
        },
    )
}

fn decode_native_result(
    return_type: ContractParameterType,
    result: Vec<u8>,
) -> CoreResult<Option<StackItem>> {
    match return_type {
        ContractParameterType::Void => Ok(None),
        ContractParameterType::Boolean => {
            let value = result.iter().any(|byte| *byte != 0);
            Ok(Some(StackItem::from_bool(value)))
        }
        ContractParameterType::Integer => {
            let big = BigInt::from_signed_bytes_le(&result);
            Ok(Some(StackItem::from_int(big)))
        }
        ContractParameterType::String => {
            let string_bytes = String::from_utf8(result)
                .map_err(|_| CoreError::other("Invalid UTF-8 string returned by native contract"))?
                .into_bytes();
            Ok(Some(StackItem::from_byte_string(string_bytes)))
        }
        ContractParameterType::Array | ContractParameterType::Map => {
            if result.is_empty() {
                // Neo native methods use an empty payload to encode `null` results
                // for stack types such as Array/Map/Any (e.g., getContract miss).
                return Ok(Some(StackItem::null()));
            }
            match BinarySerializer::deserialize(&result, &ExecutionEngineLimits::default(), None) {
                Ok(item) => Ok(Some(item)),
                Err(_) => Ok(Some(StackItem::from_byte_string(result))),
            }
        }
        ContractParameterType::Any => {
            if result.is_empty() {
                return Ok(Some(StackItem::null()));
            }
            match BinarySerializer::deserialize(&result, &ExecutionEngineLimits::default(), None) {
                Ok(item) => Ok(Some(item)),
                Err(_) => Ok(Some(StackItem::from_byte_string(result))),
            }
        }
        ContractParameterType::InteropInterface => {
            if result.len() == 4 {
                let id = BigInt::from_signed_bytes_le(&result);
                let iterator_id = id
                    .to_u32()
                    .ok_or_else(|| CoreError::other("Iterator identifier out of range"))?;
                // Iterator results are InteropInterface values (C# parity): wrap
                // the engine-side storage-iterator handle, do not surface it as a
                // bare integer.
                return Ok(Some(StackItem::from_interface(IteratorInterop::iterator(
                    iterator_id,
                ))));
            }

            // A BLS12-381 point: the native CryptoLib methods
            // (bls12381Deserialize / …Add / …Mul / …Pairing) return a point's
            // canonical encoding — 48 (G1) / 96 (G2) / 576 (Gt) bytes. Wrap it
            // as a typed interop object so a following BLS call (or
            // bls12381Serialize) accepts it while a plain byte string is
            // rejected, matching C#'s `InteropInterface` parameter binding.
            if matches!(
                result.len(),
                G1_COMPRESSED_SIZE | G2_COMPRESSED_SIZE | GT_SIZE
            ) {
                return Ok(Some(StackItem::from_interface(Bls12381Interop::bls12381(
                    result,
                ))));
            }

            Ok(Some(StackItem::from_byte_string(result)))
        }
        _ => Ok(Some(StackItem::from_byte_string(result))),
    }
}

fn stack_item_to_interop_bytes(item: StackItem) -> CoreResult<Vec<u8>> {
    // Iterator interop interfaces encode their engine-side handle id as 4 LE bytes.
    if let Ok(interface) = item.as_interface() {
        if let Some(iterator_id) = interface.iterator_id() {
            return Ok(iterator_id.to_le_bytes().to_vec());
        }
        if let Some(point) = interface.bls12381_bytes() {
            return Ok(point.to_vec());
        }
    }
    // Anything else (e.g. a plain byte string) is NOT a live interop object;
    // rejecting it matches C#, where binding an `InteropInterface` parameter
    // from a non-interface stack item throws and faults the VM.
    Err(CoreError::other("Stack item is not an InteropInterface"))
}

fn contract_native_on_persist_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let result = app.native_on_persist();
    map_contract_result("System.Contract.NativeOnPersist", result)
}

fn contract_native_post_persist_handler<P, D, B>(
    app: &mut ApplicationEngine<P, D, B>,
    _engine: &mut ExecutionEngine<B>,
) -> VmResult<()>
where
    P: NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let result = app.native_post_persist();
    map_contract_result("System.Contract.NativePostPersist", result)
}

#[cfg(test)]
#[path = "../tests/interop/application_engine_contract.rs"]
mod tests;
