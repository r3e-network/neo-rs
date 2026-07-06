//! # neo-native-contracts::std_lib
//!
//! Native StdLib string, memory, and serialization helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `encoding`: encoding and decoding routines.
//! - `invoke`: method dispatch and hardfork-gated invoke wrapper.
//! - `memory`: memory comparison/search helpers.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `numeric`: itoa/atoi and .NET integer-cast compatibility helpers.
//! - `serialization`: serialization codecs and compatibility checks.
//! - `strings`: stringSplit and strLen helpers.
//! - `tests`: Module-local tests and regression coverage.

mod encoding;
mod invoke;
mod memory;
mod metadata;
mod numeric;
mod serialization;
mod strings;

use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};

use crate::hashes::STDLIB_HASH;

/// C# `StdLib.MaxInputLength` — the `[MaxLength]` cap on string/byte inputs.
const MAX_INPUT_LENGTH: usize = 1024;

native_contract_handle!(
    /// The StdLib native contract.
    pub struct StdLib {
        id: -2,
        contract_name: "StdLib",
        hash: STDLIB_HASH,
    }
);

impl StdLib {
    fn arg_bytes<'a>(args: &'a [Vec<u8>], method: &str) -> CoreResult<&'a [u8]> {
        args.first().map(Vec::as_slice).ok_or_else(|| {
            CoreError::invalid_operation(format!("StdLib::{method} requires one argument"))
        })
    }

    fn ensure_max_len(method: &str, param: &str, value: &[u8]) -> CoreResult<()> {
        if value.len() > MAX_INPUT_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "StdLib::{method}: {param} exceeds maximum length ({MAX_INPUT_LENGTH})"
            )));
        }
        Ok(())
    }

    fn arg_bytes_max<'a>(args: &'a [Vec<u8>], method: &str, param: &str) -> CoreResult<&'a [u8]> {
        let value = Self::arg_bytes(args, method)?;
        Self::ensure_max_len(method, param, value)?;
        Ok(value)
    }

    fn arg_str_max(args: &[Vec<u8>], method: &str, param: &str) -> CoreResult<String> {
        let value = Self::arg_bytes_max(args, method, param)?;
        String::from_utf8(value.to_vec()).map_err(|_| {
            CoreError::invalid_operation(format!("StdLib::{method}: argument is not valid UTF-8"))
        })
    }

    /// Interprets the single argument as a native string (a VM ByteString carrying
    /// UTF-8 bytes).
    fn arg_str(args: &[Vec<u8>], method: &str) -> CoreResult<String> {
        String::from_utf8(Self::arg_bytes(args, method)?.to_vec()).map_err(|_| {
            CoreError::invalid_operation(format!("StdLib::{method}: argument is not valid UTF-8"))
        })
    }
}

impl NativeContract for StdLib {
    native_contract_identity!(StdLib);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::STD_LIB_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }
}

#[cfg(test)]
#[path = "../tests/std_lib/mod.rs"]
mod tests;
