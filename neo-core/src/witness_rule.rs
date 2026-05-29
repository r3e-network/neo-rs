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
pub use neo_io_crate::witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};

// Stack projection lives in neo-core (depends on the VM crate) so neo-io stays
// free of any VM dependency.
mod stack_projection;
pub use stack_projection::{ToStackItem, WitnessStackValue};

#[cfg(test)]
#[allow(dead_code)]
mod tests;
