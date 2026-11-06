// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Permission flags granted when one contract invokes another.
//!
//! `CallFlags` is canonically owned by `neo-primitives` (it is a wire-level
//! bitflag shared across layers). `neo-protocol` re-exports it so protocol-layer
//! contract and execution-adjacent code can name the type without depending on a
//! higher crate, mirroring the historical `neo-core` thin re-export.

pub use neo_primitives::call_flags::*;
