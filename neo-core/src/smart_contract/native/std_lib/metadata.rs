use super::StdLib;
use crate::hardfork::Hardfork;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ContractParameterType;

impl StdLib {
    pub(super) fn methods() -> Vec<NativeMethod> {
        let methods = vec![
            NativeMethod::safe(
                "serialize".to_string(),
                1 << 12,
                vec![ContractParameterType::Any],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "deserialize".to_string(),
                1 << 14,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Any,
            ),
            // JSON operations
            NativeMethod::safe(
                "jsonSerialize".to_string(),
                1 << 12,
                vec![ContractParameterType::Any],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "jsonDeserialize".to_string(),
                1 << 14,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Any,
            ),
            NativeMethod::safe(
                "itoa".to_string(),
                1 << 12,
                vec![ContractParameterType::Integer],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "itoa".to_string(),
                1 << 12,
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "atoi".to_string(),
                1 << 6,
                vec![ContractParameterType::String],
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "atoi".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "base64Encode".to_string(),
                1 << 5,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base64Decode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "base64UrlEncode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::String,
            )
            .with_active_in(Hardfork::HfCockatrice),
            NativeMethod::safe(
                "base64UrlDecode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::String,
            )
            .with_active_in(Hardfork::HfCockatrice),
            NativeMethod::safe(
                "base58Encode".to_string(),
                1 << 13,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base58Decode".to_string(),
                1 << 10,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "base58CheckEncode".to_string(),
                1 << 16,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base58CheckDecode".to_string(),
                1 << 16,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "hexEncode".to_string(),
                1 << 5,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            )
            .with_active_in(Hardfork::HfFaun),
            NativeMethod::safe(
                "hexDecode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            )
            .with_active_in(Hardfork::HfFaun),
            // Memory operations
            NativeMethod::safe(
                "memoryCompare".to_string(),
                1 << 5,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::Integer,
            ),
            // memorySearch overloads (2 params)
            NativeMethod::safe(
                "memorySearch".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::Integer,
            ),
            // memorySearch overloads (3 params)
            NativeMethod::safe(
                "memorySearch".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Integer,
            ),
            // memorySearch overloads (4 params)
            NativeMethod::safe(
                "memorySearch".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Integer,
            ),
            // stringSplit overloads (2 params)
            NativeMethod::safe(
                "stringSplit".to_string(),
                1 << 8,
                vec![ContractParameterType::String, ContractParameterType::String],
                ContractParameterType::Array,
            ),
            // stringSplit overloads (3 params)
            NativeMethod::safe(
                "stringSplit".to_string(),
                1 << 8,
                vec![
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Array,
            ),
            NativeMethod::safe(
                "strLen".to_string(),
                1 << 8,
                vec![ContractParameterType::String],
                ContractParameterType::Integer,
            ),
        ];
        methods
            .into_iter()
            .map(
                |method| match (method.name.as_str(), method.parameters.len()) {
                    ("atoi", 1) => method.with_parameter_names(vec!["value".to_string()]),
                    ("atoi", 2) => {
                        method.with_parameter_names(vec!["value".to_string(), "base".to_string()])
                    }
                    ("base58CheckDecode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base58CheckEncode", 1) => {
                        method.with_parameter_names(vec!["data".to_string()])
                    }
                    ("base58Decode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base58Encode", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("base64Decode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base64Encode", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("base64UrlDecode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base64UrlEncode", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("deserialize", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("hexDecode", 1) => method.with_parameter_names(vec!["str".to_string()]),
                    ("hexEncode", 1) => method.with_parameter_names(vec!["bytes".to_string()]),
                    ("itoa", 1) => method.with_parameter_names(vec!["value".to_string()]),
                    ("itoa", 2) => {
                        method.with_parameter_names(vec!["value".to_string(), "base".to_string()])
                    }
                    ("jsonDeserialize", 1) => method.with_parameter_names(vec!["json".to_string()]),
                    ("jsonSerialize", 1) => method.with_parameter_names(vec!["item".to_string()]),
                    ("memoryCompare", 2) => {
                        method.with_parameter_names(vec!["str1".to_string(), "str2".to_string()])
                    }
                    ("memorySearch", 2) => {
                        method.with_parameter_names(vec!["mem".to_string(), "value".to_string()])
                    }
                    ("memorySearch", 3) => method.with_parameter_names(vec![
                        "mem".to_string(),
                        "value".to_string(),
                        "start".to_string(),
                    ]),
                    ("memorySearch", 4) => method.with_parameter_names(vec![
                        "mem".to_string(),
                        "value".to_string(),
                        "start".to_string(),
                        "backward".to_string(),
                    ]),
                    ("serialize", 1) => method.with_parameter_names(vec!["item".to_string()]),
                    ("strLen", 1) => method.with_parameter_names(vec!["str".to_string()]),
                    ("stringSplit", 2) => method
                        .with_parameter_names(vec!["str".to_string(), "separator".to_string()]),
                    ("stringSplit", 3) => method.with_parameter_names(vec![
                        "str".to_string(),
                        "separator".to_string(),
                        "removeEmptyEntries".to_string(),
                    ]),
                    _ => method,
                },
            )
            .collect()
    }
}
