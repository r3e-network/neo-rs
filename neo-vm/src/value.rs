use alloc::string::String;

use neo_base::Bytes;

#[derive(Clone, Debug, PartialEq)]
pub enum VmValue {
    Null,
    Bool(bool),
    Int(i64),
    Bytes(Bytes),
    String(String),
}

impl VmValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            VmValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            VmValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            VmValue::Bytes(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            VmValue::String(s) => Some(s),
            _ => None,
        }
    }
}
