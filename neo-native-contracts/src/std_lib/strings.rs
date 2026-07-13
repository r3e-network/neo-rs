//! StdLib string splitting and text-element length helpers.
//!
//! Keeps UTF-8 validation, BinarySerializer return shaping, and .NET text
//! segmentation compatibility out of the contract root.

use super::{StdLib, args::MAX_INPUT_LENGTH};
use crate::text::dotnet_text_segmentation::text_element_count;
use neo_error::{CoreError, CoreResult};
use neo_serialization::BinarySerializer;
use neo_vm::StackValue;
use num_bigint::BigInt;

impl StdLib {
    /// C# `StdLib.StringSplit(str, separator[, removeEmptyEntries])` =
    /// `String.Split`: split `str` on each occurrence of `separator`, keeping
    /// empty entries unless `removeEmptyEntries` is true. An empty separator
    /// yields `[str]` (the whole string), matching .NET's `string.Split(string)`
    /// overload. Enforces the C# `[MaxLength(1024)]` cap on `str`. Returns a VM
    /// Array of ByteStrings (BinarySerialized; the engine deserializes it for
    /// the `Array` return type).
    pub(super) fn string_split_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let raw = Self::arg_bytes(args, "stringSplit")?;
        if raw.len() > MAX_INPUT_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "StdLib::stringSplit: input exceeds maximum length ({MAX_INPUT_LENGTH})"
            )));
        }
        let value = std::str::from_utf8(raw).map_err(|_| {
            CoreError::invalid_operation(
                "StdLib::stringSplit: argument is not valid UTF-8".to_string(),
            )
        })?;
        let separator = match args.get(1) {
            Some(bytes) => std::str::from_utf8(bytes).map_err(|_| {
                CoreError::invalid_operation(
                    "StdLib::stringSplit: separator is not valid UTF-8".to_string(),
                )
            })?,
            None => {
                return Err(CoreError::invalid_operation(
                    "StdLib::stringSplit requires (str, separator)".to_string(),
                ));
            }
        };
        let remove_empty = args
            .get(2)
            .map(|b| b.iter().any(|x| *x != 0))
            .unwrap_or(false);

        let parts: Vec<&str> = if separator.is_empty() {
            // .NET `string.Split("")` returns the whole string as a single element.
            vec![value]
        } else {
            value.split(separator).collect()
        };
        let items: Vec<StackValue> = parts
            .into_iter()
            .filter(|part| !remove_empty || !part.is_empty())
            .map(|part| StackValue::ByteString(part.as_bytes().to_vec()))
            .collect();

        BinarySerializer::serialize_stack_value_default(&StackValue::Array(
            neo_vm::next_stack_item_id(),
            items,
        ))
        .map_err(|e| CoreError::invalid_operation(format!("StdLib::stringSplit: {e}")))
    }

    /// C# `StdLib.StrLen(str)`: the number of text elements in the string, i.e.
    /// .NET `StringInfo` extended grapheme clusters (UAX #29 minus GB9c over the
    /// .NET runtime's break-property snapshot; see
    /// [`crate::dotnet_text_segmentation`]). Enforces the C# `[MaxLength(1024)]`
    /// cap on the raw input bytes; invalid UTF-8 faults the call, matching the
    /// C# `StrictUTF8` string conversion.
    pub(super) fn str_len_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let raw = Self::arg_bytes(args, "strLen")?;
        if raw.len() > MAX_INPUT_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "StdLib::strLen: input exceeds maximum length ({MAX_INPUT_LENGTH})"
            )));
        }
        let value = std::str::from_utf8(raw).map_err(|_| {
            CoreError::invalid_operation("StdLib::strLen: argument is not valid UTF-8".to_string())
        })?;
        Ok(BigInt::from(text_element_count(value)).to_signed_bytes_le())
    }
}
