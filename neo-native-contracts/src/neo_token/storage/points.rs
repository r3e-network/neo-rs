//! EC-point return encoders for public committee and validator arrays.

use super::*;
use neo_error::CoreError;
use neo_serialization::BinarySerializer;

impl NeoToken {
    /// Serializes EC points as an Array of compressed (33-byte) byte strings --
    /// the return shape shared by `getCommittee` / `getNextBlockValidators`.
    fn build_points_stack_item<'a, I>(points: I) -> StackItem
    where
        I: IntoIterator<Item = &'a ECPoint>,
    {
        StackItem::from_array(
            points
                .into_iter()
                .map(|p| StackItem::from_byte_string(p.to_bytes()))
                .collect::<Vec<_>>(),
        )
    }

    pub(in crate::neo_token) fn points_to_array_bytes(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
        let array = Self::build_points_stack_item(points.iter());
        BinarySerializer::serialize_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
    }

    pub(in crate::neo_token) fn points_to_stack_item<'a, I>(points: I) -> CoreResult<StackItem>
    where
        I: IntoIterator<Item = &'a ECPoint>,
    {
        Ok(Self::build_points_stack_item(points))
    }
}
