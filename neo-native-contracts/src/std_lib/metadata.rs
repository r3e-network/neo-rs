use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_execution::NativeMethod;
use neo_primitives::ContractParameterType;

pub(super) static STD_LIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let bytes = ContractParameterType::ByteArray;
    let string = ContractParameterType::String;
    let int = ContractParameterType::Integer;
    let boolean = ContractParameterType::Boolean;
    let array = ContractParameterType::Array;
    vec![
        NativeMethod::new("base64Encode", 1 << 5, true, 0, vec![bytes], string)
            .with_parameter_names(["data"]),
        NativeMethod::new("base64Decode", 1 << 5, true, 0, vec![string], bytes)
            .with_parameter_names(["s"]),
        NativeMethod::new("base58Encode", 1 << 13, true, 0, vec![bytes], string)
            .with_parameter_names(["data"]),
        NativeMethod::new("base58Decode", 1 << 10, true, 0, vec![string], bytes)
            .with_parameter_names(["s"]),
        NativeMethod::new("base58CheckEncode", 1 << 16, true, 0, vec![bytes], string)
            .with_parameter_names(["data"]),
        NativeMethod::new("base58CheckDecode", 1 << 16, true, 0, vec![string], bytes)
            .with_parameter_names(["s"]),
        // serialize(Any) -> ByteArray; deserialize(ByteArray) -> Any.
        NativeMethod::new(
            "serialize",
            1 << 12,
            true,
            0,
            vec![ContractParameterType::Any],
            bytes,
        )
        .with_parameter_names(["item"]),
        NativeMethod::new(
            "deserialize",
            1 << 14,
            true,
            0,
            vec![bytes],
            ContractParameterType::Any,
        )
        .with_parameter_names(["data"]),
        // jsonSerialize(Any) -> ByteArray; jsonDeserialize(ByteArray) -> Any
        // (C# StdLib.cs CpuFees 1<<12 / 1<<14).
        NativeMethod::new(
            "jsonSerialize",
            1 << 12,
            true,
            0,
            vec![ContractParameterType::Any],
            bytes,
        )
        .with_parameter_names(["item"]),
        NativeMethod::new(
            "jsonDeserialize",
            1 << 14,
            true,
            0,
            vec![bytes],
            ContractParameterType::Any,
        )
        .with_parameter_names(["json"]),
        NativeMethod::new("memoryCompare", 1 << 5, true, 0, vec![bytes, bytes], int)
            .with_parameter_names(["str1", "str2"]),
        // memorySearch's 3 C# overloads (dispatched by argument count).
        NativeMethod::new("memorySearch", 1 << 6, true, 0, vec![bytes, bytes], int)
            .with_parameter_names(["mem", "value"]),
        NativeMethod::new(
            "memorySearch",
            1 << 6,
            true,
            0,
            vec![bytes, bytes, int],
            int,
        )
        .with_parameter_names(["mem", "value", "start"]),
        NativeMethod::new(
            "memorySearch",
            1 << 6,
            true,
            0,
            vec![bytes, bytes, int, boolean],
            int,
        )
        .with_parameter_names(["mem", "value", "start", "backward"]),
        // itoa(value[, base]) -> String; atoi(value[, base]) -> Integer.
        // (The C# `int @base` parameter's reflection name is "base".)
        NativeMethod::new("itoa", 1 << 12, true, 0, vec![int], string)
            .with_parameter_names(["value"]),
        NativeMethod::new("itoa", 1 << 12, true, 0, vec![int, int], string)
            .with_parameter_names(["value", "base"]),
        NativeMethod::new("atoi", 1 << 6, true, 0, vec![string], int)
            .with_parameter_names(["value"]),
        NativeMethod::new("atoi", 1 << 6, true, 0, vec![string, int], int)
            .with_parameter_names(["value", "base"]),
        // stringSplit(str, separator[, removeEmptyEntries]) -> Array of String.
        NativeMethod::new("stringSplit", 1 << 8, true, 0, vec![string, string], array)
            .with_parameter_names(["str", "separator"]),
        NativeMethod::new(
            "stringSplit",
            1 << 8,
            true,
            0,
            vec![string, string, boolean],
            array,
        )
        .with_parameter_names(["str", "separator", "removeEmptyEntries"]),
        // strLen(str) -> Integer (count of .NET StringInfo text elements);
        // ungated in C# StdLib.cs, CpuFee 1 << 8.
        NativeMethod::new("strLen", 1 << 8, true, 0, vec![string], int)
            .with_parameter_names(["str"]),
        // base64Url* are available from the Echidna hardfork onward.
        NativeMethod::new("base64UrlEncode", 1 << 5, true, 0, vec![string], string)
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(["data"]),
        NativeMethod::new("base64UrlDecode", 1 << 5, true, 0, vec![string], string)
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(["s"]),
        // hexEncode/hexDecode are available from the Faun hardfork onward.
        NativeMethod::new("hexEncode", 1 << 5, true, 0, vec![bytes], string)
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(["bytes"]),
        NativeMethod::new("hexDecode", 1 << 5, true, 0, vec![string], bytes)
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(["str"]),
    ]
});
