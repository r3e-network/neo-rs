// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Witness-rule binary codec (matches C# `WitnessCondition` / `WitnessRule`).
//!
//! The protocol model, its structural validation (nesting depth, sub-item
//! limits) and its JSON/serde shape are owned by `neo-primitives`
//! (`neo_primitives::witness_rule`). Both `neo-vm` and `neo-protocol` depend on
//! that crate, so the model can be shared without the I/O crate owning
//! protocol concepts.
//!
//! What remains here is the Neo binary codec — it needs this crate's local
//! `Serializable` trait and `BinaryWriter`/`MemoryReader` — plus the ECPoint
//! group helpers that codec relies on.

/// ECPoint group-byte helpers used by the witness-rule codec.
pub mod helpers;
mod serialization;

pub use neo_primitives::WitnessConditionType;
pub use neo_primitives::{WitnessCondition, WitnessRule, WitnessRuleAction};
