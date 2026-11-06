// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Witness - Transaction signature verification for Neo N3.
//!
//! The canonical `Witness` payload is owned by `neo-protocol` and re-exported
//! here for backward compatibility (R2 leaf-type unification). All structural
//! API (`new`, `new_with_scripts`, `script_hash`, `size`, `to_json`, wire
//! `Serializable`, `Display`, `Default`, and the `neo_primitives::Witness`
//! impl) is provided by `neo-protocol`.
//!
//! neo-core keeps only the pieces that depend on the VM / crypto layer:
//! the free verification helpers in [`verification`] and the local
//! [`WitnessData`] trait implemented by verifiable containers.
//!
//! ## Example
//!
//! ```rust
//! use neo_core::Witness;
//! use neo_vm::OpCode;
//!
//! // Create a witness from scripts
//! let invocation_script = vec![OpCode::PUSHDATA1.byte(), 0x40, 0x01, 0x02];
//! let verification_script = vec![OpCode::PUSHDATA1.byte(), 0x21];
//! let witness = Witness::new_with_scripts(invocation_script, verification_script);
//!
//! // Get the script hash
//! let script_hash = witness.script_hash();
//! ```

// Witness verification helpers now live in `neo-smart-contract` (single source of
// truth); re-exported here to preserve the historical `neo_core::witness::*` path.
pub use neo_smart_contract::witness_verification::{
    WitnessExt, verify_witness_multi_signature, verify_witness_signature,
};

/// Canonical witness payload, owned by `neo-protocol`.
pub use neo_protocol::witness::Witness;

/// Protocol witness access without snapshot validation or VM execution.
pub trait WitnessData: neo_primitives::Verifiable {
    /// Gets the witnesses associated with this container.
    fn witnesses(&self) -> Vec<&Witness>;

    /// Gets mutable access to the witnesses associated with this container.
    fn witnesses_mut(&mut self) -> Vec<&mut Witness>;
}
