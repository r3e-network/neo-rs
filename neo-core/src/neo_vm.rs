// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Neo VM compatibility surface — re-exports from the standalone `neo-vm` crate.

pub use neo_vm::error;
pub use neo_vm::evaluation_stack;
pub use neo_vm::execution_context;
pub use neo_vm::execution_engine;
pub use neo_vm::interop_service;
pub use neo_vm::jump_table;
pub use neo_vm::reference_counter;
pub use neo_vm::script;
pub use neo_vm::slot;
pub use neo_vm::stack_item;
pub use neo_vm::io;

pub use neo_vm::{
    CompoundParent, EvaluationStack, ExecutionContext, ExecutionEngine, InteropService, JumpTable,
    ReferenceCounter, Script, Slot, StackItem, VmError, VmResult,
};
