use std::collections::HashMap;
use std::rc::Rc;
use serde::{Deserialize, Serialize};
use neo_base::math::I256;
use crate::buffer::Buffer;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Interop {
    //
}


#[derive(Clone, Debug,Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StackItem {
    Null,
    Boolean(bool),
    Integer(I256),
    ByteString(Vec<u8>),
    Buffer(Vec<u8>),
    Array(Vec<Rc<StackItem>>),
    Struct(Vec<Rc<StackItem>>),
    Map(HashMap<StackItem, Rc<StackItem>>),
    Pointer(usize),
    InteropInterface(Interop),
}


impl From<String> for StackItem{
    fn from(s: String) -> Self {
        Self::ByteString(s.into())
    }
}

impl From<&str> for StackItem{
    fn from(s: &str) -> Self {
        Self::ByteString(s.into())
    }
}

impl From<&[u8]> for StackItem{
    fn from(s: &[u8]) -> Self {
        Self::Buffer(s.into())
    }
}

impl From<Vec<u8>> for StackItem{
    fn from(s: Vec<u8>) -> Self {
        Self::Buffer(s.into())
    }
}

