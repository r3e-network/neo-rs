//! StdLib native contract implementation.
//!
//! The StdLib contract provides standard utility functions for smart contracts,
//! including string manipulation, JSON operations, and mathematical functions.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::impl_native_contract;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::UInt160;

mod encoding;
mod helpers;
mod memory;
mod metadata;
mod numbers;
mod serialization;
mod strings;

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

        Self {
            id: Self::ID,
            hash,
            methods: Self::methods(),
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
}

impl NativeContract for StdLib {
    impl_native_contract!(hash, "StdLib", methods);

    fn id(&self) -> i32 {
        self.id
    }
}

impl Default for StdLib {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
