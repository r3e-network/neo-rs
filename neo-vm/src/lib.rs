// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;


pub mod builder;
pub mod decode;
pub mod program;
pub mod interop;
pub mod opcode;
pub mod operand;
pub mod stack;
mod tables;

use tables::*;
pub use {builder::*, decode::*, interop::*, opcode::*, operand::*, program::*, stack::*};


pub(crate) const MAX_STACK_ITEM_SIZE: usize = 65535 * 2;


pub trait RunPrice {
    fn price(&self) -> u64;
}


pub struct NeoVm {
    //
}