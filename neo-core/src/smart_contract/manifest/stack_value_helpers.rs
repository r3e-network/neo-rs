use crate::error::CoreError;
use neo_vm_rs::StackValue;

pub(super) fn required_struct_fields(
    stack_value: StackValue,
    type_name: &str,
    required_len: usize,
) -> Result<Vec<StackValue>, CoreError> {
    let StackValue::Struct(items) = stack_value else {
        return Err(CoreError::invalid_format(format!(
            "{type_name} expects Struct stack value"
        )));
    };

    if items.len() < required_len {
        return Err(CoreError::invalid_format(format!(
            "{type_name} stack value must contain {required_len} elements, found {}",
            items.len()
        )));
    }

    Ok(items)
}

pub(super) fn decode_stack_value_objects<T>(
    stack_value: StackValue,
    mut update: impl FnMut(&mut T, StackValue) -> Result<(), CoreError>,
) -> Result<Option<Vec<T>>, CoreError>
where
    T: Default,
{
    let items = match stack_value {
        StackValue::Array(items) | StackValue::Struct(items) => items,
        _ => return Ok(None),
    };

    items
        .into_iter()
        .map(|item| {
            let mut decoded = T::default();
            update(&mut decoded, item)?;
            Ok(decoded)
        })
        .collect::<Result<Vec<_>, CoreError>>()
        .map(Some)
}
