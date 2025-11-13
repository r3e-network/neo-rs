#![cfg_attr(not(feature = "std"), no_std)]

//! Minimal stack-based virtual machine used to bootstrap the Neo N3 Rust node.
//! It executes a compact instruction set and delegates native calls to the
//! `neo-contract` layer.

extern crate alloc;

mod error;
mod instruction;
mod runtime;
mod stack_item;
mod syscall;
mod value;
mod vm;

pub use error::VmError;
pub use instruction::Instruction;
pub use runtime::{RuntimeHost, Trigger};
pub use stack_item::StackItem;
pub use syscall::SyscallDispatcher;
pub use value::VmValue;
pub use vm::{NativeInvoker, VirtualMachine};
