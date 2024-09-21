// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};
#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
use neo_base::{encoding::bin::*, math::U256};
use serde::{Deserialize, Serialize};

use crate::{
    types::{Bytes, Sign, H160, H256},
    PublicKey,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, BinEncode, BinDecode)]
pub enum ParamType {
    Any = 0x00,
    Boolean = 0x10,
    Integer = 0x11,
    ByteArray = 0x12,
    String = 0x13,
    H160 = 0x14,
    H256 = 0x15,
    PublicKey = 0x16,
    Signature = 0x17,
    Array = 0x20,
    Map = 0x22,
    InteropInterface = 0x30,
    Void = 0xff,
}

pub type ParamMap = HashMap<ParamValue, ParamValue>;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ParamValue {
    Boolean(bool),
    Integer(U256),

    ByteArray(Bytes),
    String(String),

    H160(H160),
    H256(H256),

    PublicKey(PublicKey),
    Signature(Sign),

    Array(Vec<ParamValue>),
    Map(ParamMap),

    InteropInterface,
    Void,
}

impl core::hash::Hash for ParamValue {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Boolean(v) => v.hash(state),
            Self::Integer(v) => v.hash(state),
            Self::ByteArray(v) => v.hash(state),
            Self::String(v) => v.hash(state),
            Self::H160(v) => v.hash(state),
            Self::H256(v) => v.hash(state),
            Self::PublicKey(v) => v.hash(state),
            Self::Signature(v) => v.hash(state),
            Self::Array(v) => v.hash(state),
            Self::Map(_v) => state.write_u8(0xfa),
            Self::InteropInterface => state.write_u8(0xfb),
            Self::Void => state.write_u8(0xfc),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Param {
    // `skip_serializing_if` not works.
    Any(#[serde(default, skip_serializing_if = "Option::is_none")] Option<ParamValue>),

    Boolean(bool),
    Integer(U256),

    ByteArray(Bytes),
    String(String),

    H160(H160),
    H256(H256),

    PublicKey(PublicKey),
    Signature(Sign),

    Array(Vec<Param>),
    Map(ParamMap),

    InteropInterface,
    Void,
}

impl Param {
    #[inline]
    pub fn is_null(&self) -> bool {
        match self {
            Self::Any(None) => true,
            _ => false,
        }
    }

    pub fn param_type(&self) -> ParamType {
        use ParamType::*;
        match self {
            Self::Any(_) => Any,
            Self::Boolean(_) => Boolean,
            Self::Integer(_) => Integer,
            Self::ByteArray(_) => ByteArray,
            Self::String(_) => String,
            Self::H160(_) => H160,
            Self::H256(_) => H256,
            Self::PublicKey(_) => PublicKey,
            Self::Signature(_) => Signature,
            Self::Array(_) => Array,
            Self::Map(_) => Map,
            Self::InteropInterface => InteropInterface,
            Self::Void => Void,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedParam {
    /// empty name means no name,
    #[serde(default)]
    pub name: String,

    #[serde(flatten)]
    pub value: Param,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedParamType {
    pub name: String,

    #[serde(rename = "type")]
    pub typ: ParamType,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_param_eq() {
        let mut m = ParamMap::new();
        m.insert(ParamValue::Void, ParamValue::Boolean(false));

        let v = m.get(&ParamValue::Void).expect("key should exist");
        assert_eq!(v, &ParamValue::Boolean(false));

        m.insert(ParamValue::Map(ParamMap::new()), ParamValue::Boolean(true));
        let v = m.get(&ParamValue::Map(ParamMap::new())).expect("key should exist");
        assert_eq!(v, &ParamValue::Boolean(true));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn test_param_marshal() {
        let p = Param::Boolean(true);
        let p = serde_json::to_string(&p).expect("json encode should be ok");
        assert_eq!(&p, r#"{"type":"Boolean","value":true}"#);

        let v: Param = serde_json::from_str(&p).expect("json decode should be ok");
        assert!(matches!(v, Param::Boolean(true)));

        let p = Param::Any(None);
        let p = serde_json::to_string(&p).expect("json encode should be ok");
        assert_eq!(&p, r#"{"type":"Any","value":null}"#);

        let p = NamedParam { name: "token".into(), value: Param::String("abc".into()) };
        let p = serde_json::to_string(&p).expect("json encode should be ok");
        assert_eq!(&p, r#"{"name":"token","type":"String","value":"abc"}"#);
    }
}
