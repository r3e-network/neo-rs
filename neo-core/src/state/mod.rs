// Copyright (C) 2015-2025 The Neo Project.
//
// lib.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! # Neo State
//!
//! World state abstraction for the Neo N3 blockchain.
//!
//! This crate provides a clean abstraction over blockchain state management,
//! including accounts, contract storage, and state snapshots.
//!
//! ## Features
//!
//! - **Account State**: Track NEO/GAS balances and validator votes
//! - **Contract Storage**: Key-value storage for smart contracts
//! - **Snapshots**: Point-in-time state isolation for transaction execution
//! - **Rollback**: Atomic state changes with commit/rollback semantics
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      WorldState                              │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                    StateView (read)                      ││
//! │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ││
//! │  │  │ AccountState │  │ContractStorage│  │  StorageItem │  ││
//! │  │  └──────────────┘  └──────────────┘  └──────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                    StateMut (write)                      ││
//! │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ││
//! │  │  │ StateChanges │  │  Snapshot    │  │   Rollback   │  ││
//! │  │  └──────────────┘  └──────────────┘  └──────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use neo_state::{WorldState, MemoryWorldState, StateChanges, AccountState};
//! use neo_primitives::UInt160;
//!
//! // Create in-memory state
//! let mut state = MemoryWorldState::new();
//!
//! // Create and commit account
//! let hash = UInt160::default();
//! let account = AccountState::with_balances(hash, 100, 50_000_000);
//!
//! let mut changes = StateChanges::new();
//! changes.accounts.insert(hash, Some(account));
//! state.commit(changes).unwrap();
//!
//! // Read account
//! let retrieved = state.get_account(&hash).unwrap();
//! ```
//!
//! ## Design Principles
//!
//! 1. **Storage Agnostic**: No direct dependency on `RocksDB` or other backends
//! 2. **Snapshot Isolation**: Changes are isolated until explicitly committed
//! 3. **Atomic Operations**: All-or-nothing commit semantics
//! 4. **Thread Safety**: All types are `Send + Sync`

mod account;
mod contract_storage;
mod error;
mod snapshot;
mod state_trie;
mod world_state;

pub use account::AccountState;
pub use contract_storage::{ContractStorage, StorageChange, StorageItem, StorageKey};
pub use error::{StateError, StateResult};
pub use snapshot::{SnapshotManager, SnapshotState, StateSnapshot, MAX_SNAPSHOT_DEPTH};
pub use state_trie::{MemoryMptStore, StateTrieManager};
pub use world_state::{
    MemoryWorldState, MutableStateView, StateChanges, StateMut, StateView, WorldState,
};

/// Re-export primitives for convenience.
pub mod primitives {
    pub use neo_primitives::{UInt160, UInt256};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_exports() {
        // Verify all public types are accessible
        let _account = AccountState::default();
        let _key = StorageKey::new(neo_primitives::UInt160::default(), vec![]);
        let _item = StorageItem::new(vec![]);
        let _changes = StateChanges::new();
        let _state = MemoryWorldState::new();
    }

    #[test]
    fn test_end_to_end_workflow() {
        use neo_primitives::UInt160;

        // Create state
        let mut state = MemoryWorldState::new();

        // Create account
        let hash = UInt160::default();
        let account = AccountState::with_balances(hash, 1000, 100_000_000);

        // Commit account
        let mut changes = StateChanges::new();
        changes.accounts.insert(hash, Some(account));
        state.commit(changes).unwrap();

        // Verify account
        let retrieved = state.get_account(&hash).unwrap().unwrap();
        assert_eq!(retrieved.neo_balance(), 1000);
        assert_eq!(retrieved.gas_balance(), 100_000_000);

        // Create storage
        let key = StorageKey::new(hash, vec![0x01, 0x02]);
        let item = StorageItem::new(vec![0x03, 0x04, 0x05]);

        // Commit storage
        let mut changes = StateChanges::new();
        changes.storage.insert(key.clone(), Some(item));
        state.commit(changes).unwrap();

        // Verify storage
        let retrieved = state.get_storage(&key).unwrap().unwrap();
        assert_eq!(retrieved.as_bytes(), &[0x03, 0x04, 0x05]);

        // Delete account
        let mut changes = StateChanges::new();
        changes.accounts.insert(hash, None);
        state.commit(changes).unwrap();

        // Verify deletion
        assert!(state.get_account(&hash).unwrap().is_none());
    }
}
