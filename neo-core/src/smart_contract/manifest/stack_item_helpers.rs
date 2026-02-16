//! Shared helpers for manifest `StackItem` parsing.

use crate::error::CoreError;
use crate::smart_contract::i_interoperable::IInteroperable;
use neo_vm::StackItem;

/// Extracts struct items from a stack item and validates a minimum field count.
pub(crate) fn expect_struct_items(
    stack_item: &StackItem,
    descriptor: &str,
    min_len: usize,
) -> Result<Vec<StackItem>, CoreError> {
    let StackItem::Struct(struct_item) = stack_item else {
        return Err(CoreError::invalid_format(format!(
            "{descriptor} expects Struct stack item",
        )));
    };

    let items = struct_item.items();
    if items.len() < min_len {
        return Err(CoreError::invalid_format(format!(
            "{descriptor} stack item must contain {min_len} elements, found {}",
            items.len()
        )));
    }
    Ok(items.to_vec())
}

/// Extracts array items from a stack item.
pub(crate) fn expect_array_items(
    item: &StackItem,
    descriptor: &str,
) -> Result<Vec<StackItem>, CoreError> {
    let StackItem::Array(array_item) = item else {
        return Err(CoreError::invalid_format(format!(
            "{descriptor} must be an Array",
        )));
    };
    Ok(array_item.items().to_vec())
}

/// Decodes an array stack item into interoperable values.
///
/// Returns `Ok(None)` when the input item is not an array to preserve
/// existing permissive parsing behaviour.
pub(crate) fn decode_interoperable_array<T>(item: &StackItem) -> Result<Option<Vec<T>>, CoreError>
where
    T: IInteroperable + Default,
{
    let Ok(array_items) = item.as_array() else {
        return Ok(None);
    };

    let values = array_items
        .iter()
        .map(|entry| {
            let mut parsed = T::default();
            parsed.from_stack_item(entry.clone())?;
            Ok(parsed)
        })
        .collect::<Result<Vec<_>, CoreError>>()?;
    Ok(Some(values))
}
