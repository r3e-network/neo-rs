// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo Constants
//!
//! Global constants used throughout the Neo blockchain implementation.
//!
//! All shared protocol constants are defined in `neo_primitives::constants` and
//! re-exported here for convenience. Only constants unique to `neo-core` are
//! defined directly in this module.
//!
//! ## Example
//!
//! ```rust
//! use neo_core::constants::{MAX_BLOCK_SIZE, MAINNET_MAGIC, MILLISECONDS_PER_BLOCK};
//!
//! assert_eq!(MAX_BLOCK_SIZE, 2_097_152); // 2 MB
//! assert_eq!(MAINNET_MAGIC, 0x334F454E);
//! assert_eq!(MILLISECONDS_PER_BLOCK, 15_000); // 15 seconds
//! ```

// Re-export all shared protocol constants from neo-primitives (single source of truth).
pub use neo_primitives::constants::*;

// === neo-core-specific constants ===

/// Wire-format upper bound for transaction count in a serialised block
/// (matches C# `Block.MaxTransactionsPerBlock = 0xFFFF`).
/// Used only during block deserialization; consensus validation uses
/// [`MAX_TRANSACTIONS_PER_BLOCK`] (or the runtime protocol setting).
pub const BLOCK_MAX_TX_WIRE_LIMIT: usize = 65_535; // u16::MAX

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_constants() {
        assert_eq!(MILLISECONDS_PER_BLOCK, 15_000);
        assert_eq!(MILLISECONDS_PER_HOUR, 3_600_000);
    }

    #[test]
    fn test_size_constants() {
        assert_eq!(MAX_BLOCK_SIZE, 2_097_152); // 2MB — Neo N3 ProtocolSettings.Default
        assert_eq!(MAX_TRANSACTION_SIZE, 102_400); // 100KB
    }

    #[test]
    fn test_wire_limit_distinct_from_consensus() {
        assert_eq!(BLOCK_MAX_TX_WIRE_LIMIT, 65_535);
        assert_ne!(BLOCK_MAX_TX_WIRE_LIMIT, MAX_TRANSACTIONS_PER_BLOCK);
    }
}
