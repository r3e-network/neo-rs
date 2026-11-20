//! Witness-rule stack projection.
//!
//! The projections are owned by `neo-vm`: it sits above `neo-io` (which owns
//! `WitnessCondition` / `WitnessRule`) and owns `StackValue` / `StackItem`, so
//! the trait impls stay globally unique there and `neo-io` stays free of a VM
//! dependency.
//!
//! This module only re-exports them so the historical
//! `neo_smart_contract::protocol_projection::*` paths keep resolving. Layout
//! matches C# `WitnessCondition.ToStackItem` / `WitnessRule.ToStackItem`.

pub use neo_vm::{
    ToStackItem, ToStackValue, witness_condition_to_stack_item, witness_condition_to_stack_value,
    witness_rule_to_stack_item, witness_rule_to_stack_value,
};
