//! StdLib argument decoding helpers.
//!
//! Centralizes the C# `StdLib.MaxInputLength` rule and one-argument
//! byte/string extraction so encoding, memory, numeric, serialization, and
//! string helpers can share identical validation and error shaping.

use super::StdLib;
use neo_error::{CoreError, CoreResult};

/// C# `StdLib.MaxInputLength` — the `[MaxLength]` cap on string/byte inputs.
pub(super) const MAX_INPUT_LENGTH: usize = 1024;

impl StdLib {
    pub(super) fn arg_bytes<'a>(args: &'a [Vec<u8>], method: &str) -> CoreResult<&'a [u8]> {
        args.first().map(Vec::as_slice).ok_or_else(|| {
            CoreError::invalid_operation(format!("StdLib::{method} requires one argument"))
        })
    }

    pub(super) fn ensure_max_len(method: &str, param: &str, value: &[u8]) -> CoreResult<()> {
        if value.len() > MAX_INPUT_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "StdLib::{method}: {param} exceeds maximum length ({MAX_INPUT_LENGTH})"
            )));
        }
        Ok(())
    }

    pub(super) fn arg_bytes_max<'a>(
        args: &'a [Vec<u8>],
        method: &str,
        param: &str,
    ) -> CoreResult<&'a [u8]> {
        let value = Self::arg_bytes(args, method)?;
        Self::ensure_max_len(method, param, value)?;
        Ok(value)
    }

    pub(super) fn arg_str_max(args: &[Vec<u8>], method: &str, param: &str) -> CoreResult<String> {
        let value = Self::arg_bytes_max(args, method, param)?;
        String::from_utf8(value.to_vec()).map_err(|_| {
            CoreError::invalid_operation(format!("StdLib::{method}: argument is not valid UTF-8"))
        })
    }

    /// Interprets the single argument as a native string (a VM ByteString carrying
    /// UTF-8 bytes).
    pub(super) fn arg_str(args: &[Vec<u8>], method: &str) -> CoreResult<String> {
        String::from_utf8(Self::arg_bytes(args, method)?.to_vec()).map_err(|_| {
            CoreError::invalid_operation(format!("StdLib::{method}: argument is not valid UTF-8"))
        })
    }
}
