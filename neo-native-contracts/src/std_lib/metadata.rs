use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_execution::NativeMethod;
use neo_primitives::ContractParameterType;
use neo_vm::StackItem;

use super::StdLib;
use crate::support::invoke::{NativeMethodBinding, NativeMethodHandler, method_metadata};

pub(super) const STD_LIB_METHOD_COUNT: usize = 25;

pub(super) fn std_lib_method_bindings<P, D, B>() -> Vec<NativeMethodBinding<StdLib, P, D, B>>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let bytes = ContractParameterType::ByteArray;
    let string = ContractParameterType::String;
    let int = ContractParameterType::Integer;
    let boolean = ContractParameterType::Boolean;
    let array = ContractParameterType::Array;
    vec![
        NativeMethodBinding::new(
            NativeMethod::new("base64Encode", 1 << 5, true, 0, vec![bytes], string)
                .with_parameter_names(["data"]),
            StdLib::invoke_base64_encode,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("base64Decode", 1 << 5, true, 0, vec![string], bytes)
                .with_parameter_names(["s"]),
            StdLib::invoke_base64_decode,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("base58Encode", 1 << 13, true, 0, vec![bytes], string)
                .with_parameter_names(["data"]),
            StdLib::invoke_base58_encode,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("base58Decode", 1 << 10, true, 0, vec![string], bytes)
                .with_parameter_names(["s"]),
            StdLib::invoke_base58_decode,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("base58CheckEncode", 1 << 16, true, 0, vec![bytes], string)
                .with_parameter_names(["data"]),
            StdLib::invoke_base58_check_encode,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("base58CheckDecode", 1 << 16, true, 0, vec![string], bytes)
                .with_parameter_names(["s"]),
            StdLib::invoke_base58_check_decode,
        ),
        // serialize(Any) -> ByteArray; deserialize(ByteArray) -> Any.
        NativeMethodBinding::new(
            NativeMethod::new(
                "serialize",
                1 << 12,
                true,
                0,
                vec![ContractParameterType::Any],
                bytes,
            )
            .with_parameter_names(["item"]),
            StdLib::invoke_serialize,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "deserialize",
                1 << 14,
                true,
                0,
                vec![bytes],
                ContractParameterType::Any,
            )
            .with_parameter_names(["data"]),
            StdLib::invoke_deserialize,
        ),
        // jsonSerialize(Any) -> ByteArray; jsonDeserialize(ByteArray) -> Any
        // (C# StdLib.cs CpuFees 1<<12 / 1<<14).
        NativeMethodBinding::new(
            NativeMethod::new(
                "jsonSerialize",
                1 << 12,
                true,
                0,
                vec![ContractParameterType::Any],
                bytes,
            )
            .with_parameter_names(["item"]),
            StdLib::invoke_json_serialize,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "jsonDeserialize",
                1 << 14,
                true,
                0,
                vec![bytes],
                ContractParameterType::Any,
            )
            .with_parameter_names(["json"]),
            StdLib::invoke_json_deserialize,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("memoryCompare", 1 << 5, true, 0, vec![bytes, bytes], int)
                .with_parameter_names(["str1", "str2"]),
            StdLib::invoke_memory_compare,
        ),
        // memorySearch's 3 C# overloads (dispatched by argument count).
        NativeMethodBinding::new(
            NativeMethod::new("memorySearch", 1 << 6, true, 0, vec![bytes, bytes], int)
                .with_parameter_names(["mem", "value"]),
            StdLib::invoke_memory_search,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "memorySearch",
                1 << 6,
                true,
                0,
                vec![bytes, bytes, int],
                int,
            )
            .with_parameter_names(["mem", "value", "start"]),
            StdLib::invoke_memory_search,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "memorySearch",
                1 << 6,
                true,
                0,
                vec![bytes, bytes, int, boolean],
                int,
            )
            .with_parameter_names(["mem", "value", "start", "backward"]),
            StdLib::invoke_memory_search,
        ),
        // itoa(value[, base]) -> String; atoi(value[, base]) -> Integer.
        // (The C# `int @base` parameter's reflection name is "base".)
        NativeMethodBinding::new(
            NativeMethod::new("itoa", 1 << 12, true, 0, vec![int], string)
                .with_parameter_names(["value"]),
            StdLib::invoke_itoa,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("itoa", 1 << 12, true, 0, vec![int, int], string)
                .with_parameter_names(["value", "base"]),
            StdLib::invoke_itoa,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("atoi", 1 << 6, true, 0, vec![string], int)
                .with_parameter_names(["value"]),
            StdLib::invoke_atoi,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("atoi", 1 << 6, true, 0, vec![string, int], int)
                .with_parameter_names(["value", "base"]),
            StdLib::invoke_atoi,
        ),
        // stringSplit(str, separator[, removeEmptyEntries]) -> Array of String.
        NativeMethodBinding::new(
            NativeMethod::new("stringSplit", 1 << 8, true, 0, vec![string, string], array)
                .with_parameter_names(["str", "separator"]),
            StdLib::invoke_string_split,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "stringSplit",
                1 << 8,
                true,
                0,
                vec![string, string, boolean],
                array,
            )
            .with_parameter_names(["str", "separator", "removeEmptyEntries"]),
            StdLib::invoke_string_split,
        ),
        // strLen(str) -> Integer (count of .NET StringInfo text elements);
        // ungated in C# StdLib.cs, CpuFee 1 << 8.
        NativeMethodBinding::new(
            NativeMethod::new("strLen", 1 << 8, true, 0, vec![string], int)
                .with_parameter_names(["str"]),
            StdLib::invoke_str_len,
        ),
        // base64Url* are available from the Echidna hardfork onward.
        NativeMethodBinding::new(
            NativeMethod::new("base64UrlEncode", 1 << 5, true, 0, vec![string], string)
                .with_active_in(Hardfork::HfEchidna)
                .with_parameter_names(["data"]),
            StdLib::invoke_base64_url_encode,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("base64UrlDecode", 1 << 5, true, 0, vec![string], string)
                .with_active_in(Hardfork::HfEchidna)
                .with_parameter_names(["s"]),
            StdLib::invoke_base64_url_decode,
        ),
        // hexEncode/hexDecode are available from the Faun hardfork onward.
        NativeMethodBinding::new(
            NativeMethod::new("hexEncode", 1 << 5, true, 0, vec![bytes], string)
                .with_active_in(Hardfork::HfFaun)
                .with_parameter_names(["bytes"]),
            StdLib::invoke_hex_encode,
        ),
        NativeMethodBinding::new(
            NativeMethod::new("hexDecode", 1 << 5, true, 0, vec![string], bytes)
                .with_active_in(Hardfork::HfFaun)
                .with_parameter_names(["str"]),
            StdLib::invoke_hex_decode,
        ),
    ]
}

pub(super) static STD_LIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    method_metadata(&std_lib_method_bindings::<
        neo_execution::native_contract_provider::NoNativeContractProvider,
        neo_execution::NoDiagnostic,
        neo_storage::EmptyCacheBacking,
    >())
});

#[inline]
pub(super) fn std_lib_method_handler<P, D, B>(
    index: usize,
) -> Option<NativeMethodHandler<StdLib, P, D, B>>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    if index >= STD_LIB_METHOD_COUNT {
        return None;
    }

    let handler = match index {
        0 => StdLib::invoke_base64_encode::<P, D, B>,
        1 => StdLib::invoke_base64_decode::<P, D, B>,
        2 => StdLib::invoke_base58_encode::<P, D, B>,
        3 => StdLib::invoke_base58_decode::<P, D, B>,
        4 => StdLib::invoke_base58_check_encode::<P, D, B>,
        5 => StdLib::invoke_base58_check_decode::<P, D, B>,
        6 => StdLib::invoke_serialize::<P, D, B>,
        7 => StdLib::invoke_deserialize::<P, D, B>,
        8 => StdLib::invoke_json_serialize::<P, D, B>,
        9 => StdLib::invoke_json_deserialize::<P, D, B>,
        10 => StdLib::invoke_memory_compare::<P, D, B>,
        11..=13 => StdLib::invoke_memory_search::<P, D, B>,
        14..=15 => StdLib::invoke_itoa::<P, D, B>,
        16..=17 => StdLib::invoke_atoi::<P, D, B>,
        18..=19 => StdLib::invoke_string_split::<P, D, B>,
        20 => StdLib::invoke_str_len::<P, D, B>,
        21 => StdLib::invoke_base64_url_encode::<P, D, B>,
        22 => StdLib::invoke_base64_url_decode::<P, D, B>,
        23 => StdLib::invoke_hex_encode::<P, D, B>,
        24 => StdLib::invoke_hex_decode::<P, D, B>,
        _ => return None,
    };
    Some(handler)
}

#[inline]
pub(super) fn invoke_std_lib_method_by_index<P, D, B>(
    contract: &StdLib,
    engine: &mut neo_execution::ApplicationEngine<P, D, B>,
    index: usize,
    args: &[Vec<u8>],
) -> Option<neo_error::CoreResult<Vec<u8>>>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    std_lib_method_handler(index).map(|handler| handler(contract, engine, args))
}

#[inline]
pub(super) fn invoke_std_lib_stack_item_method_by_index<P, D, B>(
    contract: &StdLib,
    engine: &mut neo_execution::ApplicationEngine<P, D, B>,
    index: usize,
    args: &[StackItem],
) -> Option<neo_error::CoreResult<Option<StackItem>>>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    match index {
        6 => Some(contract.invoke_serialize_stack_item(engine, args)),
        7 => Some(contract.invoke_deserialize_stack_item(engine, args)),
        14..=15 => Some(contract.invoke_itoa_stack_item(engine, args)),
        _ => None,
    }
}
