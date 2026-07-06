use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_execution::NativeMethod;
use neo_primitives::ContractParameterType;

use super::StdLib;
use crate::support::invoke::{NativeMethodBinding, method_metadata};

pub(super) static STD_LIB_METHOD_BINDINGS: LazyLock<Vec<NativeMethodBinding<StdLib>>> =
    LazyLock::new(|| {
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
    });

pub(super) static STD_LIB_METHODS: LazyLock<Vec<NativeMethod>> =
    LazyLock::new(|| method_metadata(&STD_LIB_METHOD_BINDINGS));
