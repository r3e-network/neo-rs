use std::convert::TryFrom;
use std::fmt;
use std::str::FromStr;
use serde::{Deserialize, Serialize};
use neo_crypto::keys;
use neo_types::{address, util};
use neo_vm::{emit, opcode, stackitem};

// ParamType represents the Type of the smart contract parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(i8)]
pub enum ParamType {
    Unknown = -1,
    Any = 0x00,
    Bool = 0x10,
    Integer = 0x11,
    ByteArray = 0x12,
    String = 0x13,
    Hash160 = 0x14,
    Hash256 = 0x15,
    PublicKey = 0x16,
    Signature = 0x17,
    Array = 0x20,
    Map = 0x22,
    InteropInterface = 0x30,
    Void = 0xff,
}

// Lengths (in bytes) of fixed-size types.
pub const HASH160_LEN: usize = util::UINT160_SIZE;
pub const HASH256_LEN: usize = util::UINT256_SIZE;
pub const PUBLIC_KEY_LEN: usize = 33;
pub const SIGNATURE_LEN: usize = keys::SIGNATURE_LEN;

// FILE_BYTES_PARAM_TYPE is a string representation of `filebytes` parameter type used in cli.
pub const FILE_BYTES_PARAM_TYPE: &str = "filebytes";

impl fmt::Display for ParamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamType::Signature => write!(f, "Signature"),
            ParamType::Bool => write!(f, "Boolean"),
            ParamType::Integer => write!(f, "Integer"),
            ParamType::Hash160 => write!(f, "Hash160"),
            ParamType::Hash256 => write!(f, "Hash256"),
            ParamType::ByteArray => write!(f, "ByteArray"),
            ParamType::PublicKey => write!(f, "PublicKey"),
            ParamType::String => write!(f, "String"),
            ParamType::Array => write!(f, "Array"),
            ParamType::Map => write!(f, "Map"),
            ParamType::InteropInterface => write!(f, "InteropInterface"),
            ParamType::Void => write!(f, "Void"),
            ParamType::Any => write!(f, "Any"),
            ParamType::Unknown => write!(f, ""),
        }
    }
}

impl ParamType {
    pub fn encode_default_value<W: io::Write>(&self, w: &mut W) -> io::Result<()> {
        let mut b = [0u8; SIGNATURE_LEN];

        match self {
            ParamType::Any | ParamType::Signature | ParamType::String | ParamType::ByteArray => {
                emit::bytes(w, &b)?;
            }
            ParamType::Bool => {
                emit::bool(w, true)?;
            }
            ParamType::Integer => {
                emit::instruction(w, opcode::PUSHINT256, &b[..32])?;
            }
            ParamType::Hash160 => {
                emit::bytes(w, &b[..HASH160_LEN])?;
            }
            ParamType::Hash256 => {
                emit::bytes(w, &b[..HASH256_LEN])?;
            }
            ParamType::PublicKey => {
                emit::bytes(w, &b[..PUBLIC_KEY_LEN])?;
            }
            ParamType::Array | ParamType::Map | ParamType::InteropInterface | ParamType::Void => {}
            ParamType::Unknown => {}
        }
        Ok(())
    }

    pub fn match_stackitem(&self, v: &stackitem::Item) -> bool {
        use stackitem::Type;

        if v.type_() == Type::Pointer {
            return false;
        }

        match self {
            ParamType::Any => true,
            ParamType::Bool => v.type_() == Type::Boolean,
            ParamType::Integer => v.type_() == Type::Integer,
            ParamType::ByteArray => matches!(v.type_(), Type::ByteArray | Type::Buffer | Type::Any),
            ParamType::String => {
                matches!(v.type_(), Type::ByteArray | Type::Buffer) && 
                String::from_utf8(v.value().to_vec()).is_ok()
            }
            ParamType::Hash160 => Self::check_bytes_with_len(v, HASH160_LEN),
            ParamType::Hash256 => Self::check_bytes_with_len(v, HASH256_LEN),
            ParamType::PublicKey => Self::check_bytes_with_len(v, PUBLIC_KEY_LEN),
            ParamType::Signature => Self::check_bytes_with_len(v, SIGNATURE_LEN),
            ParamType::Array => matches!(v.type_(), Type::Any | Type::Array | Type::Struct),
            ParamType::Map => matches!(v.type_(), Type::Any | Type::Map),
            ParamType::InteropInterface => matches!(v.type_(), Type::Any | Type::Interop),
            _ => false,
        }
    }

    fn check_bytes_with_len(v: &stackitem::Item, l: usize) -> bool {
        match v.type_() {
            stackitem::Type::Any => true,
            stackitem::Type::ByteArray | stackitem::Type::Buffer => {
                v.try_bytes().map(|b| b.len() == l).unwrap_or(false)
            }
            _ => false,
        }
    }

    pub fn convert_to_stackitem_type(&self) -> stackitem::Type {
        match self {
            ParamType::Signature | ParamType::Hash160 | ParamType::Hash256 | 
            ParamType::ByteArray | ParamType::PublicKey | ParamType::String => stackitem::Type::ByteArray,
            ParamType::Bool => stackitem::Type::Boolean,
            ParamType::Integer => stackitem::Type::Integer,
            ParamType::Array => stackitem::Type::Array,
            ParamType::Map => stackitem::Type::Map,
            ParamType::InteropInterface => stackitem::Type::Interop,
            ParamType::Void | ParamType::Any => stackitem::Type::Any,
            ParamType::Unknown => panic!("unknown param type"),
        }
    }
}

impl FromStr for ParamType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "signature" => Ok(ParamType::Signature),
            "bool" | "boolean" => Ok(ParamType::Bool),
            "int" | "integer" => Ok(ParamType::Integer),
            "hash160" => Ok(ParamType::Hash160),
            "hash256" => Ok(ParamType::Hash256),
            "bytes" | "bytearray" | "bytestring" | FILE_BYTES_PARAM_TYPE => Ok(ParamType::ByteArray),
            "key" | "publickey" => Ok(ParamType::PublicKey),
            "string" => Ok(ParamType::String),
            "array" | "struct" => Ok(ParamType::Array),
            "map" => Ok(ParamType::Map),
            "interopinterface" => Ok(ParamType::InteropInterface),
            "void" => Ok(ParamType::Void),
            "any" => Ok(ParamType::Any),
            _ => Err(format!("bad parameter type: {}", s)),
        }
    }
}

impl TryFrom<i8> for ParamType {
    type Error = String;

    fn try_from(value: i8) -> Result<Self, Self::Error> {
        match value {
            -1 => Ok(ParamType::Unknown),
            0x00 => Ok(ParamType::Any),
            0x10 => Ok(ParamType::Bool),
            0x11 => Ok(ParamType::Integer),
            0x12 => Ok(ParamType::ByteArray),
            0x13 => Ok(ParamType::String),
            0x14 => Ok(ParamType::Hash160),
            0x15 => Ok(ParamType::Hash256),
            0x16 => Ok(ParamType::PublicKey),
            0x17 => Ok(ParamType::Signature),
            0x20 => Ok(ParamType::Array),
            0x22 => Ok(ParamType::Map),
            0x30 => Ok(ParamType::InteropInterface),
            0xff => Ok(ParamType::Void),
            _ => Err(format!("unknown parameter type: {}", value)),
        }
    }
}

// Note: The `adjust_val_to_type` and `infer_param_type` functions are not directly translated
// as they involve complex type conversions and external dependencies. These would need to be
// implemented separately, considering Rust's type system and available libraries.
