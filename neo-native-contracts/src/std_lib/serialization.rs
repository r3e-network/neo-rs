//! StdLib BinarySerializer and JSON bridge helpers.

use neo_error::{CoreError, CoreResult};
use neo_serialization::{BinarySerializer, JsonSerializer};
use neo_vm_rs::ExecutionEngineLimits;

use super::StdLib;

/// serialize(item) -> the item's BinarySerializer bytes. The `Any`-typed arg is
/// already BinarySerialized by the engine, so C# `BinarySerializer.Serialize`
/// is exactly `args[0]`.
pub(super) fn serialize_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_bytes(args, "serialize").map(<[u8]>::to_vec)
}

/// deserialize(data) -> the StackItem. Validate malformed input here, then hand
/// the bytes back for the engine's Any-return decode.
pub(super) fn deserialize_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_bytes(args, "deserialize").and_then(|data| {
        BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
            .map(|_| data.to_vec())
            .map_err(|e| CoreError::invalid_operation(format!("StdLib::deserialize: {e}")))
    })
}

/// jsonSerialize(item) -> JSON bytes (System.Text.Json byte-exact).
pub(super) fn json_serialize_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    StdLib::arg_bytes(args, "jsonSerialize").and_then(|data| {
        let limits = ExecutionEngineLimits::default();
        let item = BinarySerializer::deserialize(data, &limits, None)
            .map_err(|e| CoreError::invalid_operation(format!("StdLib::jsonSerialize: {e}")))?;
        JsonSerializer::serialize_to_byte_array(&item, limits.max_item_size)
            .map_err(|e| CoreError::invalid_operation(format!("StdLib::jsonSerialize: {e}")))
    })
}

/// jsonDeserialize(json) -> the StackItem, re-encoded as BinarySerializer bytes.
/// Depth 10 + MaxStackSize match C# (`JToken.Parse(json, 10)` + engine limits).
///
/// `basilisk_active` is `engine.IsHardforkEnabled(Hardfork.HF_Basilisk)` for the
/// current block, gating the JSON-number → Integer conversion exactly as C#
/// `JsonSerializer.Deserialize` does: pre-Basilisk numbers use the `(BigInteger)double`
/// truncating cast (`1e30` -> 1000000000000000019884624838656), post-Basilisk parse
/// the decimal string (`1e30` -> 10^30). Replaying a block below the Basilisk height
/// with the wrong flag diverges from C# on any number whose magnitude exceeds 2^53.
pub(super) fn json_deserialize_impl(
    args: &[Vec<u8>],
    basilisk_active: bool,
) -> CoreResult<Vec<u8>> {
    StdLib::arg_bytes(args, "jsonDeserialize").and_then(|json| {
        let limits = ExecutionEngineLimits::default();
        let item =
            JsonSerializer::deserialize(json, 10, limits.max_stack_size as usize, basilisk_active)
                .map_err(|e| {
                    CoreError::invalid_operation(format!("StdLib::jsonDeserialize: {e}"))
                })?;
        BinarySerializer::serialize(&item, &limits)
            .map_err(|e| CoreError::invalid_operation(format!("StdLib::jsonDeserialize: {e}")))
    })
}
