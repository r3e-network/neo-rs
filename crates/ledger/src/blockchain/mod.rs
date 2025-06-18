//! Blockchain module.
//!
//! This module provides the main blockchain functionality exactly matching C# Neo Blockchain.
//! The module is organized following C# Neo's blockchain structure:
//! - storage: Storage interface and implementation (matches C# Storage classes)
//! - genesis: Genesis block creation and initialization (matches C# Genesis handling)
//! - verification: Block, header, and transaction verification (matches C# verification logic)
//! - persistence: Block persistence and storage management (matches C# persistence layer)
//! - state: Blockchain state management (matches C# state handling)
//! - blockchain: Main Blockchain struct (matches C# Blockchain class)

pub mod storage;
pub mod genesis;
pub mod verification;
pub mod persistence;
pub mod state;
pub mod blockchain;

// Re-export main types for compatibility
pub use blockchain::{Blockchain, BlockchainStats};
pub use storage::{Storage, StorageKey, StorageItem, StorageProvider, RocksDBStorage};
pub use genesis::GenesisManager;
pub use verification::{BlockchainVerifier, VerifyResult};
pub use persistence::{BlockchainPersistence, BlockchainSnapshot};
pub use state::{BlockchainState, ContractState, PolicySettings}; 