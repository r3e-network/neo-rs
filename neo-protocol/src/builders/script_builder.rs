// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Programmatic VM script construction for the protocol layer.
//!
//! The concrete builder lives in `neo-vm` (it is intrinsically tied to the VM
//! opcode set and integer encoding rules). `neo-protocol` re-exports it so that
//! protocol-layer builders — contract redeem-script construction and witness
//! script helpers — can emit scripts without depending on any higher crate
//! (`neo-core`, `neo-smart-contract`, ...).
//!
//! This mirrors the historical `neo-core` surface (`pub use neo_vm::ScriptBuilder`)
//! and keeps a single canonical implementation.

pub use neo_vm::ScriptBuilder;
