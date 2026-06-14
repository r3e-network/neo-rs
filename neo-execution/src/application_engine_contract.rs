//! ApplicationEngine.Contract - ports Neo.SmartContract.ApplicationEngine.Contract.cs

use crate::application_engine::ApplicationEngine;
use crate::bls12381_interop::Bls12381Interop;
use crate::env_flags::env_flag_enabled;
use crate::execution_context_state::ExecutionContextState;
use crate::iterators::IteratorInterop;
use neo_crypto::bls12381_point::{G1_COMPRESSED_SIZE, G2_COMPRESSED_SIZE, GT_SIZE};
use neo_error::{CoreError, CoreResult};
use neo_manifest::CallFlags;
use neo_primitives::ContractParameterType;
use neo_primitives::UInt160;
use neo_serialization::BinarySerializer;
use neo_vm::{ExecutionEngine, StackItem, VmError, VmResult};
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::sync::OnceLock;

const SYSTEM_CONTRACT_CALL_PRICE: i64 = 1 << 15;

/// Bitmask tracking which native-call args were originally `StackItem::Null`.
///
/// Bit `i` (LSB) is set if arg index `i` was popped as `StackItem::Null`.
/// The dispatcher in [`contract_call_native_handler`] populates this state
/// before invoking the native method, and the handler can query it via
/// `ApplicationEngine::get_state::<NativeArgNullMask>()` to distinguish
/// `Null` from `ByteString("")` (both of which collapse to ambiguous bytes
/// at the `Vec<u8>` args layer — see `OracleContract::request` filter handling).
pub struct NativeArgNullMask(pub u32);

/// Per-call marker set by a native method to force its return value to
/// `StackItem::Null`, regardless of the (non-`Void`) ABI return type.
///
/// This lets a method whose C# signature is a nullable reference (e.g.
/// `byte[]?` for `CryptoLib.recoverSecp256K1`) return `null` through the
/// `Vec<u8>` result channel, which otherwise cannot distinguish an empty byte
/// string from `null`. The dispatcher in [`contract_call_native_handler`]
/// consumes this marker right after the native call and, when present, pushes
/// `Null` instead of decoding the (empty) result payload.
pub(crate) struct NativeReturnNull;

impl ApplicationEngine {
    /// Signals that the currently-executing native method returns `null` (for a
    /// nullable-reference return such as `CryptoLib.recoverSecp256K1`). The method
    /// should still return `Ok(Vec::new())`; the dispatcher pushes `StackItem::Null`.
    pub fn set_native_return_null(&mut self) {
        self.set_state(NativeReturnNull);
    }
}

fn native_call_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| env_flag_enabled("NEO_TRACE_CALL_NATIVE", false))
}

impl ApplicationEngine {
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

fn contract_get_call_flags_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = (|| -> CoreResult<()> {
        let flags = app.get_current_call_flags()?;
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

fn contract_create_multisig_account_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
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
        if m < 0 || m > i32::MAX as i64 {
            return Err(CoreError::other("Invalid multisig threshold"));
        }

        let account = app
            .create_multisig_account(m as i32, public_keys_items)?;
        app.push_bytes(account.to_bytes())?;
        Ok(())
    })();

    map_contract_result("System.Contract.CreateMultisigAccount", result)
}

fn contract_call_native_handler(
    app: &mut ApplicationEngine,
    engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = (|| -> CoreResult<()> {
        let version_item = engine.pop().map_err(|e| CoreError::other(e.to_string()))?;
        let version_big = version_item.as_int().map_err(|e| CoreError::other(e.to_string()))?;
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
            let state_arc = context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
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

        if native_call_trace_enabled() {
            let caller_hash = app.get_calling_script_hash().unwrap_or_else(UInt160::zero);
            let current_hash = app.current_script_hash().unwrap_or_else(UInt160::zero);
            eprintln!(
                "call_native begin contract={} method={} arg_count={} param_types={:?} current={} caller={}",
                script_hash, method_name, arg_count, parameter_types, current_hash, caller_hash
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
            if native_call_trace_enabled() {
                let stack_type = item.stack_item_type();
                let bytes_len = item.as_bytes().map(|value| value.len());
                eprintln!(
                    "call_native stack_item[{index}] vm_type={stack_type:?} as_bytes_len={bytes_len:?}"
                );
            }
            let bytes = match parameter_types.get(index) {
                Some(ContractParameterType::Any) => {
                    BinarySerializer::serialize(&item, app.execution_limits())?
                }
                Some(ContractParameterType::InteropInterface) => stack_item_to_interop_bytes(item)?,
                _ => ApplicationEngine::stack_item_to_bytes(item)?,
            };
            if native_call_trace_enabled() {
                let preview_len = bytes.len().min(24);
                eprintln!(
                    "call_native arg[{index}] type={:?} len={} preview=0x{}",
                    parameter_types.get(index),
                    bytes.len(),
                    hex::encode(&bytes[..preview_len])
                );
            }
            args.push(bytes);
        }

        app.set_state(NativeArgNullMask(null_mask));
        let call_result = app.call_native_contract(script_hash, &method_name, &args);
        app.take_state::<NativeArgNullMask>();
        let result_bytes = call_result?;
        // A native method may signal a `null` return (a nullable-reference result
        // such as `byte[]?`) via `set_native_return_null`; consume it here so it
        // never leaks into the next call.
        let force_null_return = app.take_state::<NativeReturnNull>().is_some();

        if native_call_trace_enabled() {
            let preview_len = result_bytes.len().min(24);
            eprintln!(
                "call_native result contract={} method={} len={} preview=0x{}",
                script_hash,
                method_name,
                result_bytes.len(),
                hex::encode(&result_bytes[..preview_len])
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
                engine.push(StackItem::null()).map_err(|e| CoreError::other(e.to_string()))?;
            } else {
                push_native_result(engine, ret_type, result_bytes)?;
            }
        }

        // Load any queued calls requested by the native method (e.g. NEP-17 callbacks).
        app.process_pending_native_calls()?;

        Ok(())
    })();

    map_contract_result("System.Contract.CallNative", result)
}

fn push_native_result(
    engine: &mut ExecutionEngine,
    return_type: ContractParameterType,
    result: Vec<u8>,
) -> CoreResult<()> {
    let Some(item) = decode_native_result(return_type, result)? else {
        return Ok(());
    };
    engine.push(item).map_err(|e| CoreError::other(e.to_string()))
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
            let string_bytes = String::from_utf8(result.clone())
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
                return Ok(Some(StackItem::from_interface(IteratorInterop::new(
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
                return Ok(Some(StackItem::from_interface(Bls12381Interop::new(
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
    if let Ok(iterator) = item.as_interface::<IteratorInterop>() {
        return Ok(iterator.id().to_le_bytes().to_vec());
    }
    // BLS12-381 points carry their canonical encoding directly.
    if let Ok(point) = item.as_interface::<Bls12381Interop>() {
        return Ok(point.bytes().to_vec());
    }
    // Anything else (e.g. a plain byte string) is NOT a live interop object;
    // rejecting it matches C#, where binding an `InteropInterface` parameter
    // from a non-interface stack item throws and faults the VM.
    Err(CoreError::other("Stack item is not an InteropInterface"))
}

fn contract_native_on_persist_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = app.native_on_persist();
    map_contract_result("System.Contract.NativeOnPersist", result)
}

fn contract_native_post_persist_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let result = app.native_post_persist();
    map_contract_result("System.Contract.NativePostPersist", result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_native_result_array_empty_is_null() {
        let item = decode_native_result(ContractParameterType::Array, Vec::new())
            .expect("decode")
            .expect("stack item");
        assert!(item.is_null());
    }

    #[test]
    fn decode_native_result_any_empty_is_null() {
        let item = decode_native_result(ContractParameterType::Any, Vec::new())
            .expect("decode")
            .expect("stack item");
        assert!(item.is_null());
    }

    #[test]
    fn decode_native_result_any_invalid_payload_preserves_raw_bytes() {
        let item = decode_native_result(ContractParameterType::Any, vec![0xff])
            .expect("decode")
            .expect("stack item");
        assert_eq!(item.as_bytes().expect("bytes"), vec![0xff]);
    }

    #[test]
    fn decode_native_result_any_deserializes_stack_item_payloads() {
        let original = StackItem::from_array(vec![StackItem::from_int(BigInt::from(1u8))]);
        let encoded = BinarySerializer::serialize(&original, &ExecutionEngineLimits::default())
            .expect("encode");
        let decoded = decode_native_result(ContractParameterType::Any, encoded)
            .expect("decode")
            .expect("stack item");
        assert!(matches!(decoded, StackItem::Array(_)));
    }

    #[test]
    fn decode_native_result_array_payload_roundtrips() {
        let original = StackItem::from_array(vec![StackItem::from_int(BigInt::from(1u8))]);
        let encoded = BinarySerializer::serialize(&original, &ExecutionEngineLimits::default())
            .expect("encode");
        let decoded = decode_native_result(ContractParameterType::Array, encoded)
            .expect("decode")
            .expect("stack item");
        assert!(matches!(decoded, StackItem::Array(_)));
    }

    #[test]
    fn decode_native_result_interop_wraps_bls_point_lengths() {
        // A 4-byte InteropInterface payload is an iterator handle.
        let iter = decode_native_result(ContractParameterType::InteropInterface, vec![1, 0, 0, 0])
            .expect("decode")
            .expect("stack item");
        assert!(iter.as_interface::<IteratorInterop>().is_ok());

        // 48 / 96 / 576-byte payloads are BLS12-381 points → Bls12381Interop.
        for len in [G1_COMPRESSED_SIZE, G2_COMPRESSED_SIZE, GT_SIZE] {
            let bytes = vec![0u8; len];
            let item = decode_native_result(ContractParameterType::InteropInterface, bytes.clone())
                .expect("decode")
                .expect("stack item");
            let point = item
                .as_interface::<Bls12381Interop>()
                .expect("BLS interop wrapper");
            assert_eq!(point.bytes(), bytes.as_slice());
            // It is NOT an iterator (the two interop kinds are distinct).
            assert!(item.as_interface::<IteratorInterop>().is_err());
        }
    }

    #[test]
    fn interop_bytes_round_trips_typed_objects_and_rejects_plain_bytestring() {
        // A Bls12381Interop operand unwraps back to its canonical bytes.
        let bytes = vec![7u8; GT_SIZE];
        let item = StackItem::from_interface(Bls12381Interop::new(bytes.clone()));
        assert_eq!(stack_item_to_interop_bytes(item).expect("bls bytes"), bytes);

        // An IteratorInterop operand encodes its handle id as 4 LE bytes.
        let iter = StackItem::from_interface(IteratorInterop::new(5));
        assert_eq!(
            stack_item_to_interop_bytes(iter).expect("iter id"),
            5u32.to_le_bytes()
        );

        // A plain ByteString is NOT a live interop object: C# faults when binding
        // an InteropInterface parameter from a non-interface item, so we must err
        // rather than silently accept the raw bytes.
        let raw = StackItem::from_byte_string(vec![0u8; GT_SIZE]);
        assert!(stack_item_to_interop_bytes(raw).is_err());
    }
}
