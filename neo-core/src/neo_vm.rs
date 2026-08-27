// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Neo VM compatibility surface — re-exports from the self-contained `neo-vm` crate.
//!
//! `neo-vm` now vendors the full VM (native `StackItem` engine + the ABI/semantics
//! layer previously provided by the external `neo-vm-rs` crate). A glob re-export
//! keeps this compatibility module in sync with `neo-vm`'s public surface so
//! `crate::neo_vm::<Symbol>` resolves for every VM type used across neo-core.

pub use neo_vm::*;
