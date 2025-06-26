// Copyright (C) 2015-2025 The Neo Project.
//
// mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Transaction module for Neo blockchain transactions.
//!
//! This module provides the core Transaction implementation that matches
//! the C# Neo N3 implementation exactly, broken down into logical components:
//!
//! - `transaction` - Core Transaction struct and basic operations
//! - `attributes` - Transaction attributes (HighPriority, Oracle, etc.)
//! - `validation` - Transaction verification and validation logic
//! - `blockchain` - Blockchain state integration (snapshots, storage)
//! - `vm` - VM integration (ApplicationEngine, VMState)

pub mod attributes;
pub mod blockchain;
pub mod core;
pub mod serialization;
pub mod validation;
pub mod vm;

// Re-export main types for convenience
pub use attributes::*;
pub use blockchain::*;
pub use core::*;
pub use vm::*;

// Re-export constants
pub use core::{HEADER_SIZE, MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE};
