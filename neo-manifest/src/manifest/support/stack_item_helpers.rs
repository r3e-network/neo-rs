use neo_error::CoreError;
use neo_primitives::ContractParameterType;
use neo_vm::StackItem;
use num_traits::ToPrimitive;

pub(super) fn required_struct_fields(
    stack_item: StackItem,
    type_name: &str,
    required_len: usize,
) -> Result<Vec<StackItem>, CoreError> {
    let StackItem::Struct(structure) = stack_item else {
        return Err(CoreError::invalid_format(format!(
            "{type_name} expects Struct stack item"
        )));
    };
    let items = structure.items();

    if items.len() < required_len {
        return Err(CoreError::invalid_format(format!(
            "{type_name} stack item must contain {required_len} elements, found {}",
            items.len()
        )));
    }

    Ok(items)
}

pub(super) fn decode_stack_item_objects<T>(
    stack_item: StackItem,
    mut update: impl FnMut(&mut T, StackItem) -> Result<(), CoreError>,
) -> Result<Vec<T>, CoreError>
where
    T: Default,
{
    let StackItem::Array(array) = stack_item else {
        return Err(CoreError::invalid_format(
            "Contract descriptor list must be an Array",
        ));
    };

    array
        .items()
        .into_iter()
        .map(|item| {
            let mut decoded = T::default();
            update(&mut decoded, item)?;
            Ok(decoded)
        })
        .collect::<Result<Vec<_>, CoreError>>()
}

pub(super) fn stack_item_to_utf8_string(
    value: &StackItem,
    field_name: &str,
) -> Result<String, CoreError> {
    if value.is_null() {
        return Err(CoreError::invalid_format(format!(
            "{field_name} must not be null"
        )));
    }
    let bytes = value.as_bytes().map_err(|_| {
        CoreError::invalid_format(format!("{field_name} must be string-compatible"))
    })?;
    String::from_utf8(bytes)
        .map_err(|_| CoreError::invalid_format(format!("{field_name} must be valid UTF-8")))
}

pub(super) fn stack_item_to_u8(value: &StackItem, field_name: &str) -> Result<u8, CoreError> {
    let integer = value.as_int().map_err(|_| {
        CoreError::invalid_format(format!("{field_name} must be integer-compatible"))
    })?;
    integer
        .to_u8()
        .ok_or_else(|| CoreError::invalid_format(format!("{field_name} must fit UInt8")))
}

pub(super) fn stack_item_to_i32(value: &StackItem, field_name: &str) -> Result<i32, CoreError> {
    let integer = value.as_int().map_err(|_| {
        CoreError::invalid_format(format!("{field_name} must be integer-compatible"))
    })?;
    integer
        .to_i32()
        .ok_or_else(|| CoreError::invalid_format(format!("{field_name} must fit Int32")))
}

pub(super) fn stack_item_to_parameter_type(
    value: &StackItem,
    field_name: &str,
) -> Result<ContractParameterType, CoreError> {
    let byte = stack_item_to_u8(value, field_name)?;
    ContractParameterType::from_byte(byte)
        .ok_or_else(|| CoreError::invalid_format(format!("{field_name} is not a defined type")))
}

pub(super) fn json_string_to_parameter_type(
    value: &str,
    field_name: &str,
) -> Result<ContractParameterType, CoreError> {
    for candidate in ContractParameterType::all() {
        if candidate.as_str() == value {
            return Ok(candidate);
        }
    }

    if let Ok(byte) = value.parse::<u8>() {
        return ContractParameterType::from_byte(byte)
            .ok_or_else(|| CoreError::other(format!("{field_name} is not a defined type")));
    }

    Err(CoreError::other(format!(
        "{field_name} is not a defined type"
    )))
}
