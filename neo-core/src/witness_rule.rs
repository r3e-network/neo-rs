// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! WitnessRule - Conditional witness validation for Neo N3.
//!
//! The canonical implementation lives in `neo-io::witness_rule`. This module
//! re-exports the core types and adds VM-specific extensions (stack projection)
//! that depend on `neo-vm`.

// Re-export core types from neo-io
pub use neo_io_crate::witness_rule::{
    WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction,
};

// Stack projection now lives in neo-vm (single source of truth); re-exported
// here to preserve the historical `witness_rule::{ToStackItem, ToStackValue}` path.
pub use neo_vm::stack_projection::{
    ToStackItem, ToStackValue, witness_condition_to_stack_value, witness_rule_to_stack_value,
};

#[cfg(test)]
#[allow(dead_code)]
mod tests;
