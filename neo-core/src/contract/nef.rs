// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{string::String, vec::Vec};
use primitive_types::H160;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr as DeserializeRepr, Serialize_repr as SerializeRepr};

use neo_base::{errors, encoding::bin::*, hash::Sha256Checksum};
use neo_vm::vm::script::Script;

pub const NEF3_MAGIC: u32 = 0x3346454E;
pub const MAX_METHOD_LENGTH: usize = 32;


#[derive(Debug, Copy, Clone, Eq, PartialEq, SerializeRepr, DeserializeRepr, BinEncode, BinDecode)]
#[repr(u8)]
#[bin(repr = u8)]
pub enum CallFlags {
    None = 0x00,

    ReadStates = 0x01,
    WriteStates = 0x02,

    AllowCall = 0x04,
    AllowNotify = 0x08,

    /// ReadStates | WriteStates
    States = 0x03,

    /// ReadStates | AllowCall
    ReadOnly = 0x05,

    /// States | AllowCall | AllowNotify
    All = 0x0F,
}

#[derive(Debug, Clone, Serialize, Deserialize, BinEncode, BinDecode)]
pub struct MethodToken {
    pub hash: H160,

    /// `method` cannot start with '_'
    pub method: String,

    pub param_count: u16,

    #[serde(rename = "hasreturnvalue")]
    pub has_return: bool,

    #[serde(rename = "callflags")]
    pub call_flags: CallFlags,
}


/// Neo Executable File, Version 3. NEP16
#[derive(Debug, Serialize, Deserialize, BinEncode, BinDecode)]
pub struct Nef3 {
    /// Magic number for version 3, 0x3346454E
    pub magic: u32,

    /// Compiler name and version
    pub compiler: FixedBytes<64>,

    /// The url of the source files
    pub source: String,

    /// Must be 0
    #[serde(skip)]
    reserve1: u8,

    /// Method tokens, and these to be called statically
    pub tokens: Vec<MethodToken>,

    /// Must be 0
    #[serde(skip)]
    reserve2: u16,

    pub script: Script,

    pub checksum: u32,
}


impl Nef3 {
    pub fn calc_checksum(&mut self) -> u32 {
        let bin = self.to_bin_encoded();
        let checksum = (&bin[..bin.len() - 4]).sha256_checksum();

        self.checksum = checksum;
        checksum
    }
}

#[derive(Debug, Clone, Eq, PartialEq, errors::Error)]
pub enum Nef3Error {
    #[error("nef3: magic must be 0x3346454E, but actual is {0}")]
    InvalidMagic(u32),

    #[error("nef3: field '{0}, is too long({1} > {2})")]
    FieldTooLong(&'static str, u32, u32),

    #[error("nef3: invalid value or length of field '{0}'")]
    InvalidField(&'static str),

    #[error("nef3: invalid checksum {0} != {1}")]
    InvalidChecksum(u32, u32),
}

impl Nef3 {
    pub fn is_valid(&self) -> Result<(), Nef3Error> {
        if self.magic != NEF3_MAGIC {
            return Err(Nef3Error::InvalidMagic(self.magic));
        }

        let len = self.source.len();
        if len > 256 {
            return Err(Nef3Error::FieldTooLong("source", len as u32, 256));
        }

        let len = self.tokens.len();
        if len > 128 {
            return Err(Nef3Error::FieldTooLong("tokens", len as u32, 128));
        }

        if self.script.len() == 0 {
            return Err(Nef3Error::InvalidField("script"));
        }

        if self.reserve1 != 0 || self.reserve2 != 0 {
            return Err(Nef3Error::InvalidField("reserve"));
        }

        if core::str::from_utf8(self.compiler.as_ref()).is_err() {
            return Err(Nef3Error::InvalidField("compiler"));
        }

        let bin = self.to_bin_encoded();
        let checksum = (&bin[..bin.len() - 4]).sha256_checksum();
        if checksum != self.checksum {
            return Err(Nef3Error::InvalidChecksum(checksum, self.checksum));
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use alloc::vec;
    use crate::neo_contract::call_flags::CallFlags;

    #[test]
    fn test_nef_serializing() {
        let flag = serde_json::to_string(&CallFlags::AllowCall)
            .expect("json encode should be ok");
        assert_eq!(&flag, "4");

        let flag: CallFlags = serde_json::from_str(&flag)
            .expect("json decode should be ok");
        assert_eq!(flag, CallFlags::AllowCall);

        let _ = serde_json::from_str::<CallFlags>("255")
            .expect_err("json decode should be ok");
    }

    #[test]
    fn test_nef_checking() {
        let mut f = Nef3 {
            magic: NEF3_MAGIC,
            compiler: "v1".as_bytes().into(),
            source: "".into(),
            reserve1: 0,
            tokens: vec![
                MethodToken {
                    hash: Default::default(),
                    method: "method".into(),
                    param_count: 3,
                    has_return: true,
                    call_flags: CallFlags::WriteStates,
                }
            ],
            reserve2: 0,
            script: [12u8, 32, 84, 35, 14].as_slice().into(),
            checksum: 0,
        };

        let checksum = f.is_valid().expect_err("NEF3 should be invalid");
        assert!(matches!(checksum, Nef3Error::InvalidChecksum(_, _)));

        let checksum = f.calc_checksum();
        assert_eq!(checksum, 1901545998);
    }
}