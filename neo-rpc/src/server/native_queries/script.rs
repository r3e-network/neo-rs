//! Dynamic-call script construction for native-contract read probes.
//!
//! The query facade decides which native method to call. This module owns the
//! bytecode layout for C# `ScriptBuilderExtensions.EmitDynamicCall`.

use neo_error::{CoreError, CoreResult};
use neo_primitives::CallFlags;
use neo_primitives::UInt160;
use neo_vm::script_builder::ScriptBuilder;

/// Argument value for a native-contract probe call.
pub(crate) enum NativeArg<'a> {
    /// Raw byte-string argument (hashes, public keys, ...).
    Bytes(&'a [u8]),
    /// Integer argument.
    Int(i64),
}

/// Builds a read-only dynamic-call script for `method` on `contract`.
pub(crate) fn build_native_call_script(
    contract: &UInt160,
    method: &str,
    args: &[NativeArg<'_>],
) -> CoreResult<Vec<u8>> {
    let mut builder = ScriptBuilder::new();
    emit_native_call(&mut builder, contract, method, args)?;
    Ok(builder.to_array())
}

/// Emits a dynamic call to `method` on `contract` with the given
/// arguments and `CallFlags::READ_ONLY`, mirroring C#
/// `ScriptBuilderExtensions.EmitDynamicCall`: push the argument array
/// (reversed, then `PACK`), then call flags, method name, contract
/// hash, and the `System.Contract.Call` syscall.
fn emit_native_call(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
    args: &[NativeArg<'_>],
) -> CoreResult<()> {
    if args.is_empty() {
        builder.emit_push_int(0);
        builder.emit_pack();
    } else {
        for arg in args.iter().rev() {
            match arg {
                NativeArg::Bytes(bytes) => {
                    builder.emit_push(bytes);
                }
                NativeArg::Int(value) => {
                    builder.emit_push_int(*value);
                }
            }
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_pack();
    }
    builder.emit_push_int(i64::from(CallFlags::READ_ONLY.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&contract.to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| CoreError::other(err.to_string()))?;
    Ok(())
}
