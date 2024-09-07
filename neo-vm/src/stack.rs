// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::rc::Rc;

use hashbrown::HashMap;
use num_enum::TryFromPrimitive;

use neo_base::math::I256;


#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
pub enum StackItemType {
    Any = 0x00,
    Pointer = 0x10,
    Boolean = 0x20,
    Integer = 0x21,

    // i.e. readonly bytes
    ByteString = 0x28,

    // i.e. bytes
    Buffer = 0x30,
    Array = 0x40,
    Struct = 0x41,
    Map = 0x48,
    InteropInterface = 0x60,
}


pub struct Interop {
    //
}


pub enum StackItem {
    Null,
    Boolean(bool),
    Integer(I256),
    ByteString(Vec<u8>),
    Buffer(Vec<u8>),
    Array(Vec<Rc<StackItem>>),
    Struct(Vec<Rc<StackItem>>),
    Map(HashMap<StackItem, Rc<StackItem>>),
    InteropInterface(Interop),
}
