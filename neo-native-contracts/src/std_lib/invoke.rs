//! StdLib native-method dispatch.
//!
//! Keeps the method-name routing and hardfork-gated invoke wrapper separate
//! from the string, memory, and serialization implementations.

use super::{StdLib, encoding, serialization};
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use num_bigint::BigInt;

impl StdLib {
    pub(super) fn invoke_native(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        crate::support::invoke::dispatch_by_name_and_arity(
            self,
            &super::metadata::STD_LIB_METHOD_BINDINGS,
            engine,
            method,
            args,
        )
        .unwrap_or_else(|| {
            Err(CoreError::invalid_operation(format!(
                "StdLib method '{method}' is not implemented"
            )))
        })
    }

    pub(super) fn invoke_base64_encode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base64_encode_impl(args)
    }

    pub(super) fn invoke_base64_decode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base64_decode_impl(args)
    }

    pub(super) fn invoke_base64_url_encode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base64_url_encode_impl(args)
    }

    pub(super) fn invoke_base64_url_decode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base64_url_decode_impl(args)
    }

    pub(super) fn invoke_hex_encode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::hex_encode_impl(args)
    }

    pub(super) fn invoke_hex_decode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::hex_decode_impl(args)
    }

    pub(super) fn invoke_base58_encode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base58_encode_impl(args)
    }

    pub(super) fn invoke_base58_decode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base58_decode_impl(args)
    }

    pub(super) fn invoke_base58_check_encode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base58_check_encode_impl(args)
    }

    pub(super) fn invoke_base58_check_decode(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        encoding::base58_check_decode_impl(args)
    }

    pub(super) fn invoke_memory_compare(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::memory_compare_impl(args)
    }

    pub(super) fn invoke_memory_search(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::memory_search_impl(args)
    }

    pub(super) fn invoke_itoa(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::itoa_impl(args)
    }

    pub(super) fn invoke_atoi(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::atoi_impl(args)
    }

    pub(super) fn invoke_string_split(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::string_split_impl(args)
    }

    pub(super) fn invoke_str_len(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::str_len_impl(args)
    }

    pub(super) fn invoke_serialize(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        serialization::serialize_impl(args)
    }

    pub(super) fn invoke_deserialize(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        serialization::deserialize_impl(args)
    }

    pub(super) fn invoke_json_serialize(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        serialization::json_serialize_impl(args)
    }

    pub(super) fn invoke_json_deserialize(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // The only block-height-dependent StdLib behavior is jsonDeserialize's
        // number handling, gated on HF_Basilisk (C# JsonSerializer.Deserialize).
        serialization::json_deserialize_impl(args, engine.is_hardfork_enabled(Hardfork::HfBasilisk))
    }

    /// memoryCompare(a, b) -> Math.Sign(a.SequenceCompareTo(b)) as Integer.
    /// Rust slice `cmp` is the same lexicographic-then-length ordering.
    pub(in crate::std_lib) fn memory_compare_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        match (args.first(), args.get(1)) {
            (Some(a), Some(b)) => {
                match (
                    Self::ensure_max_len("memoryCompare", "str1", a),
                    Self::ensure_max_len("memoryCompare", "str2", b),
                ) {
                    (Err(e), _) | (_, Err(e)) => Err(e),
                    (Ok(()), Ok(())) => {
                        let sign: i32 = match a.as_slice().cmp(b.as_slice()) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        };
                        Ok(BigInt::from(sign).to_signed_bytes_le())
                    }
                }
            }
            _ => Err(CoreError::invalid_operation(
                "StdLib::memoryCompare requires two arguments".to_string(),
            )),
        }
    }
}
