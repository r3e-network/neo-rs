//! EC-point return encoders for public committee and validator arrays.

use super::*;
use neo_error::CoreError;
use neo_serialization::BinarySerializer;

impl NeoToken {
    /// Serializes EC points as an Array of compressed (33-byte) byte strings --
    /// the return shape shared by `getCommittee` / `getNextBlockValidators`.
    pub(in crate::neo_token) fn points_to_stack_value<'a, I>(points: I) -> StackValue
    where
        I: IntoIterator<Item = &'a ECPoint>,
    {
        StackValue::Array(
            neo_vm_rs::next_stack_item_id(),
            points
                .into_iter()
                .map(|p| StackValue::ByteString(p.to_bytes()))
                .collect::<Vec<_>>(),
        )
    }

    pub(in crate::neo_token) fn points_to_array_bytes(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
        let array = Self::points_to_stack_value(points.iter());
        BinarySerializer::serialize_stack_value_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
    }

    pub(in crate::neo_token) fn points_to_stack_item<'a, I>(points: I) -> CoreResult<StackItem>
    where
        I: IntoIterator<Item = &'a ECPoint>,
    {
        StackItem::try_from(Self::points_to_stack_value(points))
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
    }
}
