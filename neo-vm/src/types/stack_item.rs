use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use serde::{Deserialize, Serialize};
use neo_base::math::I256;
use crate::buffer::Buffer;
use crate::reference_counter::ReferenceCounter;

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

impl Default for StackItem{
    fn default() -> Self {
        StackItem::Null
    }
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

impl StackItem {
    pub fn new_array(reference_counter: Option<Rc<RefCell<ReferenceCounter>>>, items: Vec<Rc<StackItem>>) -> Self {
        let array = items.into_iter()
            .map(|item| {
                if let Some(rc) = &reference_counter {
                    rc.borrow_mut().add_reference(Rc::clone(&item));
                }
                item
            })
            .collect();
        StackItem::Array(array)
    }

    pub fn new_struct(reference_counter: Option<Rc<RefCell<ReferenceCounter>>>, items: Vec<Rc<StackItem>>) -> Self {
        let struct_items = items.into_iter()
            .map(|item| {
                if let Some(rc) = &reference_counter {
                    rc.borrow_mut().add_reference(Rc::clone(&item));
                }
                item
            })
            .collect();
        StackItem::Struct(struct_items)
    }
}
