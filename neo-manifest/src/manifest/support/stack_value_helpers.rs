use neo_error::CoreError;
use neo_primitives::ContractParameterType;
use neo_vm::StackValue;

pub(super) fn required_struct_fields(
    stack_value: StackValue,
    type_name: &str,
    required_len: usize,
) -> Result<Vec<StackValue>, CoreError> {
    let StackValue::Struct(_, items) = stack_value else {
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
) -> Result<Vec<T>, CoreError>
where
    T: Default,
{
    let StackValue::Array(_, items) = stack_value else {
        return Err(CoreError::invalid_format(
            "Contract descriptor list must be an Array",
        ));
    };

    items
        .into_iter()
        .map(|item| {
            let mut decoded = T::default();
            update(&mut decoded, item)?;
            Ok(decoded)
        })
        .collect::<Result<Vec<_>, CoreError>>()
}

pub(super) fn stack_value_to_utf8_string(
    value: &StackValue,
    field_name: &str,
) -> Result<String, CoreError> {
    if matches!(value, StackValue::Null) {
        return Err(CoreError::invalid_format(format!(
            "{field_name} must not be null"
        )));
    }
    let bytes = value.to_byte_string_bytes().ok_or_else(|| {
        CoreError::invalid_format(format!("{field_name} must be string-compatible"))
    })?;
    String::from_utf8(bytes)
        .map_err(|_| CoreError::invalid_format(format!("{field_name} must be valid UTF-8")))
}

pub(super) fn stack_value_to_u8(value: &StackValue, field_name: &str) -> Result<u8, CoreError> {
    let integer = value.to_i128().ok_or_else(|| {
        CoreError::invalid_format(format!("{field_name} must be integer-compatible"))
    })?;
    u8::try_from(integer)
        .map_err(|_| CoreError::invalid_format(format!("{field_name} must fit UInt8")))
}

pub(super) fn stack_value_to_i32(value: &StackValue, field_name: &str) -> Result<i32, CoreError> {
    let integer = value.to_i128().ok_or_else(|| {
        CoreError::invalid_format(format!("{field_name} must be integer-compatible"))
    })?;
    i32::try_from(integer)
        .map_err(|_| CoreError::invalid_format(format!("{field_name} must fit Int32")))
}

pub(super) fn stack_value_to_parameter_type(
    value: &StackValue,
    field_name: &str,
) -> Result<ContractParameterType, CoreError> {
    let byte = stack_value_to_u8(value, field_name)?;
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
