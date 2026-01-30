//! StdLib native contract implementation.
//!
//! The StdLib contract provides standard utility functions for smart contracts,
//! including string manipulation, JSON operations, and mathematical functions.

use crate::cryptography::{Base58, Hex};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::{BinarySerializer, ContractParameterType, JsonSerializer};
use crate::UInt160;
use base64::{engine::general_purpose, Engine as _};
use neo_vm::StackItem;
use num_bigint::{BigInt, Sign};
use num_traits::{Num, ToPrimitive, Zero};
use unicode_segmentation::UnicodeSegmentation;

/// The StdLib native contract.
pub struct StdLib {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl StdLib {
    const ID: i32 = -2;
    const MAX_INPUT_LENGTH: usize = 1024;

    /// Creates a new StdLib contract.
    pub fn new() -> Self {
        // StdLib contract hash: 0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0
        let hash = UInt160::parse("0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0")
            .expect("Valid StdLib contract hash");

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
            .with_active_in(Hardfork::HfEchidna),
            NativeMethod::safe(
                "base64UrlDecode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::String,
            )
            .with_active_in(Hardfork::HfEchidna),
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
        let methods = methods
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
            .collect();

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    /// Invokes a method on the StdLib contract.
    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "serialize" => self.serialize(engine, args),
            "deserialize" => self.deserialize(engine, args),
            "jsonSerialize" => self.json_serialize(engine, args),
            "jsonDeserialize" => self.json_deserialize(engine, args),
            "atoi" => self.atoi(args),
            "itoa" => self.itoa(args),
            "base64Encode" => self.base64_encode(args),
            "base64Decode" => self.base64_decode(args),
            "base64UrlEncode" => self.base64_url_encode(args),
            "base64UrlDecode" => self.base64_url_decode(args),
            "base58Encode" => self.base58_encode(args),
            "base58Decode" => self.base58_decode(args),
            "base58CheckEncode" => self.base58_check_encode(args),
            "base58CheckDecode" => self.base58_check_decode(args),
            "hexEncode" => self.hex_encode(args),
            "hexDecode" => self.hex_decode(args),
            "memoryCompare" => self.memory_compare(args),
            "memorySearch" => self.memory_search(args),
            "stringSplit" => self.string_split(engine, args),
            "strLen" => self.str_len(args),
            // Legacy alias for backward compatibility
            "stringLen" => self.str_len(args),
            _ => Err(Error::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    /// Converts a string to an integer.
    fn atoi(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "atoi requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "atoi")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        let base = self.parse_optional_base(args, 1, "atoi")?;
        let value = match base {
            10 => string_data
                .parse::<BigInt>()
                .map_err(|_| Error::native_contract("Invalid number format"))?,
            16 => self.parse_hex_twos_complement(&string_data)?,
            _ => return Err(Error::native_contract(format!("Invalid base: {}", base))),
        };

        Ok(value.to_signed_bytes_le())
    }

    /// Converts an integer to a string.
    fn itoa(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "itoa requires integer argument".to_string(),
            ));
        }

        let value = BigInt::from_signed_bytes_le(&args[0]);
        let base = self.parse_optional_base(args, 1, "itoa")?;
        let encoded = match base {
            10 => value.to_string(),
            16 => self.format_hex_twos_complement(&value),
            _ => return Err(Error::native_contract(format!("Invalid base: {}", base))),
        };

        Ok(encoded.into_bytes())
    }

    /// Serializes a stack item using the binary serializer.
    fn serialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "serialize requires data argument".to_string(),
            ));
        }

        let item = self.decode_stack_item(engine, &args[0])?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("Serialize failed: {e}")))
    }

    /// Deserializes a binary-serialized stack item.
    fn deserialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "deserialize requires data argument".to_string(),
            ));
        }

        let item = BinarySerializer::deserialize(&args[0], engine.execution_limits(), None)
            .map_err(|e| Error::native_contract(format!("Deserialize failed: {e}")))?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("Deserialize failed: {e}")))
    }

    /// Serializes a stack item to JSON.
    fn json_serialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonSerialize requires data argument".to_string(),
            ));
        }

        let item = self.decode_stack_item(engine, &args[0])?;
        JsonSerializer::serialize_to_byte_array(&item, engine.execution_limits().max_item_size)
            .map_err(|e| Error::native_contract(format!("JSON serialization error: {e}")))
    }

    /// Deserializes JSON into a stack item.
    fn json_deserialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonDeserialize requires JSON string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "jsonDeserialize")?;
        let item = JsonSerializer::deserialize(&args[0], 10)
            .map_err(|e| Error::native_contract(format!("JSON deserialization error: {e}")))?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("JSON deserialization error: {e}")))
    }

    /// Encodes data to base64.
    fn base64_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64Encode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64Encode")?;
        let encoded = general_purpose::STANDARD.encode(&args[0]);
        Ok(encoded.into_bytes())
    }

    /// Decodes data from base64.
    fn base64_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64Decode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64Decode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        let normalized: String = string_data.chars().filter(|c| !c.is_whitespace()).collect();
        let decoded = general_purpose::STANDARD
            .decode(normalized.as_bytes())
            .map_err(|_| Error::native_contract("Invalid base64 data"))?;

        Ok(decoded)
    }

    /// Encodes a string to base64url.
    fn base64_url_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64UrlEncode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64UrlEncode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        let encoded = general_purpose::URL_SAFE_NO_PAD.encode(string_data.as_bytes());
        Ok(encoded.into_bytes())
    }

    /// Decodes a string from base64url.
    fn base64_url_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64UrlDecode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base64UrlDecode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        let normalized: String = string_data.chars().filter(|c| !c.is_whitespace()).collect();
        let decoded = general_purpose::URL_SAFE_NO_PAD
            .decode(normalized.as_bytes())
            .map_err(|_| Error::native_contract("Invalid base64url data"))?;
        let decoded_string = String::from_utf8(decoded)
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Ok(decoded_string.into_bytes())
    }

    /// Encodes bytes to base58.
    fn base58_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58Encode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58Encode")?;
        Ok(Base58::encode(&args[0]).into_bytes())
    }

    /// Decodes a base58 string to bytes.
    fn base58_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58Decode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58Decode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Base58::decode(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid base58 data: {e}")))
    }

    /// Encodes bytes to base58check.
    fn base58_check_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58CheckEncode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58CheckEncode")?;
        Ok(Base58::encode_check(&args[0]).into_bytes())
    }

    /// Decodes a base58check string to bytes.
    fn base58_check_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base58CheckDecode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "base58CheckDecode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Base58::decode_check(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid base58check data: {e}")))
    }

    /// Encodes bytes to hex.
    fn hex_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "hexEncode requires data argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "hexEncode")?;
        Ok(Hex::encode(&args[0]).into_bytes())
    }

    /// Decodes hex string to bytes.
    fn hex_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "hexDecode requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "hexDecode")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        Hex::decode(&string_data)
            .map_err(|e| Error::native_contract(format!("Invalid hex data: {e}")))
    }

    /// Compares two memory regions.
    fn memory_compare(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "memoryCompare requires two data arguments".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "memoryCompare")?;
        self.ensure_max_input_len(&args[1], "memoryCompare")?;
        let result = match args[0].cmp(&args[1]) {
            std::cmp::Ordering::Less => -1i32,
            std::cmp::Ordering::Equal => 0i32,
            std::cmp::Ordering::Greater => 1i32,
        };

        Ok(result.to_le_bytes().to_vec())
    }

    /// Searches for a pattern in memory.
    /// Supports 3 overloads:
    /// - memorySearch(mem, value) -> searches from start, forward
    /// - memorySearch(mem, value, start) -> searches from start index, forward
    /// - memorySearch(mem, value, start, backward) -> searches with direction control
    fn memory_search(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "memorySearch requires at least 2 arguments (mem, value)".to_string(),
            ));
        }

        let mem = &args[0];
        let value = &args[1];

        self.ensure_max_input_len(mem, "memorySearch")?;
        self.ensure_max_input_len(value, "memorySearch")?;

        // Parse optional start parameter (default: 0)
        let start = if args.len() >= 3 {
            let start_value = BigInt::from_signed_bytes_le(&args[2]);
            start_value
                .to_i32()
                .ok_or_else(|| Error::native_contract("start parameter out of range"))?
        } else {
            0
        };

        // Parse optional backward parameter (default: false)
        let backward = if args.len() >= 4 {
            if args[3].is_empty() {
                false
            } else {
                args[3][0] != 0
            }
        } else {
            false
        };

        // Validate start index
        if start < 0 || start as usize > mem.len() {
            return Err(Error::native_contract(format!(
                "start index {} out of range [0, {}]",
                start,
                mem.len()
            )));
        }

        let start_usize = start as usize;

        // Handle empty pattern
        if value.is_empty() {
            return Ok(start.to_le_bytes().to_vec());
        }

        let result = if backward {
            // Backward search: search in mem[0..start] from end to beginning
            if start_usize < value.len() {
                -1
            } else {
                let search_range = &mem[0..start_usize];
                match search_range
                    .windows(value.len())
                    .rposition(|window| window == value)
                {
                    Some(pos) => pos as i32,
                    None => -1,
                }
            }
        } else {
            // Forward search: search in mem[start..] from beginning to end
            if start_usize + value.len() > mem.len() {
                -1
            } else {
                let search_range = &mem[start_usize..];
                match search_range
                    .windows(value.len())
                    .position(|window| window == value)
                {
                    Some(pos) => (start_usize + pos) as i32,
                    None => -1,
                }
            }
        };

        Ok(result.to_le_bytes().to_vec())
    }

    /// Splits a string by a delimiter.
    /// Supports 2 overloads:
    /// - stringSplit(str, separator) -> splits string, keeps empty entries
    /// - stringSplit(str, separator, removeEmptyEntries) -> splits with option to remove empty entries
    fn string_split(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "stringSplit requires at least 2 arguments (str, separator)".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        let separator = String::from_utf8(args[1].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 separator"))?;

        self.ensure_max_input_len(string_data.as_bytes(), "stringSplit")?;
        self.ensure_max_input_len(separator.as_bytes(), "stringSplit")?;

        // Parse optional removeEmptyEntries parameter (default: false)
        let remove_empty_entries = if args.len() >= 3 {
            if args[2].is_empty() {
                false
            } else {
                args[2][0] != 0
            }
        } else {
            false
        };

        // Split the string
        let parts: Vec<&str> = if remove_empty_entries {
            string_data
                .split(&separator)
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            string_data.split(&separator).collect()
        };

        let items = parts
            .into_iter()
            .map(|part| StackItem::from_byte_string(part.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        let array_item = StackItem::from_array(items);
        BinarySerializer::serialize(&array_item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("stringSplit failed: {e}")))
    }

    /// Gets the length of a string in grapheme clusters (text elements).
    /// This matches C#'s TextElementEnumerator behavior, correctly counting
    /// complex Unicode characters like emojis as single elements.
    /// For example: "🦆" = 1, "ã" = 1
    fn str_len(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "strLen requires string argument".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        self.ensure_max_input_len(string_data.as_bytes(), "strLen")?;

        // Count grapheme clusters (extended grapheme clusters) to match C# TextElementEnumerator
        let length = string_data.graphemes(true).count() as i32;
        Ok(length.to_le_bytes().to_vec())
    }

    fn ensure_max_input_len(&self, data: &[u8], method: &str) -> Result<()> {
        if data.len() > Self::MAX_INPUT_LENGTH {
            return Err(Error::native_contract(format!(
                "{} input exceeds max length {}",
                method,
                Self::MAX_INPUT_LENGTH
            )));
        }
        Ok(())
    }

    fn parse_optional_base(&self, args: &[Vec<u8>], index: usize, method: &str) -> Result<i32> {
        if args.len() <= index {
            return Ok(10);
        }
        let base = BigInt::from_signed_bytes_le(&args[index]);
        base.to_i32()
            .ok_or_else(|| Error::native_contract(format!("Invalid base argument for {}", method)))
    }

    fn parse_hex_twos_complement(&self, input: &str) -> Result<BigInt> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(Error::native_contract(
                "Invalid hex number format".to_string(),
            ));
        }
        if trimmed.starts_with('+') || trimmed.starts_with('-') {
            return Err(Error::native_contract(
                "Invalid hex number format".to_string(),
            ));
        }

        let normalized = trimmed.to_ascii_lowercase();
        let unsigned = BigInt::from_str_radix(&normalized, 16)
            .map_err(|_| Error::native_contract("Invalid hex number format"))?;
        let bits = trimmed
            .len()
            .checked_mul(4)
            .ok_or_else(|| Error::native_contract("Hex value too large"))?;
        if bits == 0 {
            return Ok(BigInt::from(0));
        }

        let sign_bit = BigInt::from(1) << (bits - 1);
        if (&unsigned & &sign_bit) != BigInt::from(0) {
            let modulus = BigInt::from(1) << bits;
            Ok(unsigned - modulus)
        } else {
            Ok(unsigned)
        }
    }

    fn format_hex_twos_complement(&self, value: &BigInt) -> String {
        if value.is_zero() {
            return "0".to_string();
        }
        if value.sign() != Sign::Minus {
            return value.to_str_radix(16);
        }

        let abs_value = (-value).to_biguint().unwrap_or_default();
        let bit_len = abs_value.to_str_radix(2).len();
        let is_power_of_two = !abs_value.is_zero() && (&abs_value & (&abs_value - 1u32)).is_zero();
        let bits_required = if is_power_of_two {
            bit_len
        } else {
            bit_len + 1
        };
        let nibbles = bits_required.div_ceil(4);
        let bits = nibbles * 4;
        let modulus = BigInt::from(1) << bits;
        let unsigned = modulus + value;
        let mut hex = unsigned.to_str_radix(16);
        if hex.len() < nibbles {
            let padding = "0".repeat(nibbles - hex.len());
            hex = format!("{}{}", padding, hex);
        }
        hex
    }

    fn decode_stack_item(&self, engine: &ApplicationEngine, data: &[u8]) -> Result<StackItem> {
        let limits = engine.execution_limits();
        match BinarySerializer::deserialize(data, limits, None) {
            Ok(item) => Ok(item),
            Err(_) => Ok(StackItem::from_byte_string(data.to_vec())),
        }
    }
}

impl NativeContract for StdLib {
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "StdLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for StdLib {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::DataCache;
    use crate::protocol_settings::ProtocolSettings;
    use crate::smart_contract::trigger_type::TriggerType;
    use num_bigint::BigInt;
    use std::sync::Arc;

    fn create_stdlib() -> StdLib {
        StdLib::new()
    }

    fn make_engine() -> ApplicationEngine {
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            400_000_000,
            None,
        )
        .expect("engine")
    }

    #[test]
    fn test_memory_compare() {
        let stdlib = create_stdlib();

        // Equal arrays
        let result = stdlib
            .memory_compare(&[vec![1, 2, 3], vec![1, 2, 3]])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            0
        );

        // First less than second
        let result = stdlib
            .memory_compare(&[vec![1, 2, 3], vec![1, 2, 4]])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );

        // First greater than second
        let result = stdlib
            .memory_compare(&[vec![1, 2, 4], vec![1, 2, 3]])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            1
        );

        // Different lengths
        let result = stdlib.memory_compare(&[vec![1, 2], vec![1, 2, 3]]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );
    }

    #[test]
    fn test_memory_search_basic() {
        let stdlib = create_stdlib();

        // Basic forward search
        let mem = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let pattern = vec![4, 5, 6];
        let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );

        // Pattern not found
        let pattern = vec![9, 10];
        let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );

        // Empty pattern
        let pattern = vec![];
        let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            0
        );
    }

    #[test]
    fn test_memory_search_with_start() {
        let stdlib = create_stdlib();

        let mem = vec![1, 2, 3, 4, 5, 4, 5, 6];
        let pattern = vec![4, 5];

        // Search from start=0
        let start = 0i32.to_le_bytes().to_vec();
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );

        // Search from start=4 (should find second occurrence)
        let start = 4i32.to_le_bytes().to_vec();
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            5
        );

        // Search from start=6 (should not find)
        let start = 6i32.to_le_bytes().to_vec();
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );
    }

    #[test]
    fn test_memory_search_backward() {
        let stdlib = create_stdlib();

        let mem = vec![1, 2, 3, 4, 5, 4, 5, 6];
        let pattern = vec![4, 5];

        // Backward search from start=8 (search in [0..8])
        let start = 8i32.to_le_bytes().to_vec();
        let backward = vec![1u8]; // true
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start, backward])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            5
        );

        // Backward search from start=5 (search in [0..5], should find first occurrence)
        let start = 5i32.to_le_bytes().to_vec();
        let backward = vec![1u8];
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start, backward])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );

        // Backward search from start=3 (search in [0..3], should not find)
        let start = 3i32.to_le_bytes().to_vec();
        let backward = vec![1u8];
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start, backward])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );
    }

    #[test]
    fn test_string_split_basic() {
        let stdlib = create_stdlib();
        let engine = make_engine();

        let string = "hello,world,test".as_bytes().to_vec();
        let separator = ",".as_bytes().to_vec();
        let result = stdlib.string_split(&engine, &[string, separator]).unwrap();
        let item = BinarySerializer::deserialize(&result, engine.execution_limits(), None).unwrap();
        let parts = item.as_array().unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            String::from_utf8(parts[0].as_bytes().unwrap()).unwrap(),
            "hello"
        );
        assert_eq!(
            String::from_utf8(parts[1].as_bytes().unwrap()).unwrap(),
            "world"
        );
        assert_eq!(
            String::from_utf8(parts[2].as_bytes().unwrap()).unwrap(),
            "test"
        );
    }

    #[test]
    fn test_string_split_with_empty_entries() {
        let stdlib = create_stdlib();
        let engine = make_engine();

        let string = "hello,,world,,test".as_bytes().to_vec();
        let separator = ",".as_bytes().to_vec();

        // Without removeEmptyEntries (default: false)
        let result = stdlib
            .string_split(&engine, &[string.clone(), separator.clone()])
            .unwrap();
        let item = BinarySerializer::deserialize(&result, engine.execution_limits(), None).unwrap();
        let parts = item.as_array().unwrap();
        assert_eq!(parts.len(), 5); // hello, "", world, "", test

        // With removeEmptyEntries = true
        let remove_empty = vec![1u8];
        let result = stdlib
            .string_split(&engine, &[string.clone(), separator.clone(), remove_empty])
            .unwrap();
        let item = BinarySerializer::deserialize(&result, engine.execution_limits(), None).unwrap();
        let parts = item.as_array().unwrap();
        assert_eq!(parts.len(), 3); // hello, world, test
    }

    #[test]
    fn test_str_len_basic() {
        let stdlib = create_stdlib();

        // ASCII string
        let string = "hello".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            5
        );

        // Empty string
        let string = "".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            0
        );
    }

    #[test]
    fn test_str_len_unicode() {
        let stdlib = create_stdlib();

        // Emoji (should count as 1 grapheme cluster)
        let string = "🦆".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            1
        );

        // Combining character (should count as 1 grapheme cluster)
        let string = "ã".as_bytes().to_vec(); // a + combining tilde
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            1
        );

        // Mixed ASCII and emoji
        let string = "hello🦆world".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            11
        );

        // Multiple emojis
        let string = "🦆🦆🦆".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );
    }

    #[test]
    fn test_atoi_itoa() {
        let stdlib = create_stdlib();

        // Test itoa
        let number = 12345i64.to_le_bytes().to_vec();
        let result = stdlib.itoa(&[number]).unwrap();
        let string = String::from_utf8(result).unwrap();
        assert_eq!(string, "12345");

        // Test atoi
        let string = "12345".as_bytes().to_vec();
        let result = stdlib.atoi(&[string]).unwrap();
        let number = BigInt::from_signed_bytes_le(&result);
        assert_eq!(number, BigInt::from(12345));

        // Test negative number
        let number = (-12345i64).to_le_bytes().to_vec();
        let result = stdlib.itoa(&[number]).unwrap();
        let string = String::from_utf8(result).unwrap();
        assert_eq!(string, "-12345");

        // Hex negative formatting/parsing parity with C#
        let number = (-1i64).to_le_bytes().to_vec();
        let base = 16i64.to_le_bytes().to_vec();
        let result = stdlib.itoa(&[number, base.clone()]).unwrap();
        let string = String::from_utf8(result).unwrap();
        assert_eq!(string, "f");

        let string = "ff".as_bytes().to_vec();
        let result = stdlib.atoi(&[string, base]).unwrap();
        let number = BigInt::from_signed_bytes_le(&result);
        assert_eq!(number, BigInt::from(-1));
    }

    #[test]
    fn test_base64_encode_decode() {
        let stdlib = create_stdlib();

        let data = b"Hello, World!".to_vec();

        // Encode
        let encoded = stdlib.base64_encode(std::slice::from_ref(&data)).unwrap();
        let encoded_str = String::from_utf8(encoded.clone()).unwrap();
        assert_eq!(encoded_str, "SGVsbG8sIFdvcmxkIQ==");

        // Decode
        let decoded = stdlib.base64_decode(&[encoded]).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_json_serialize_deserialize() {
        let stdlib = create_stdlib();
        let engine = make_engine();

        let data = "test string".as_bytes().to_vec();

        // Serialize
        let serialized = stdlib
            .json_serialize(&engine, std::slice::from_ref(&data))
            .unwrap();
        let json_str = String::from_utf8(serialized.clone()).unwrap();
        assert!(json_str.contains("test string"));

        // Deserialize
        let deserialized = stdlib.json_deserialize(&engine, &[serialized]).unwrap();
        let decoded = BinarySerializer::deserialize(&deserialized, engine.execution_limits(), None)
            .expect("deserialize");
        assert_eq!(decoded.as_bytes().unwrap(), data);
    }
}
