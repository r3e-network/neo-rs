// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use core::hash::{Hash};

use num_enum::TryFromPrimitive;

pub const MAX_INT_SIZE: usize = 32;


#[derive(Debug, Copy, Clone, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
pub enum StackItemType {
    Any        = 0x00,
    Pointer    = 0x10,
    Boolean    = 0x20,
    Integer    = 0x21,
    ByteArray = 0x28,
    Buffer     = 0x30,
    Array      = 0x40,
    Struct     = 0x41,
    Map        = 0x48,
    InteropInterface = 0x60,
}

impl StackItemType {
    pub fn is_valid(tp: u8) -> bool {
        match tp {
            0x00 | 0x10 | 0x20 | 0x21 | 0x28 | 0x30 | 0x40 | 0x41 | 0x48 | 0x60 => true,
            _ => false,
        }
    }

    pub fn is_primitive(tp: u8) -> bool {
        match tp {
            0x20 | 0x21 | 0x28 => true,
            _ => false,
        }
    }

    pub fn is_compound(tp: u8) -> bool {
        match tp {
            0x40 | 0x41 | 0x48 => true,
            _ => false,
        }
    }
}
