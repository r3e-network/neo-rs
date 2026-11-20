//! Blockchain command types, relay results, pre-persistence admission policy
//! and block cache retention policy.

pub mod admission;
pub mod cache_policy;
pub mod types;

// Re-export the blockchain-module surface for convenience
pub use admission::{BlockAdmissionPort, PersistenceAdmission, admit_block_for_persistence};
pub use cache_policy::{
    BlockCachePolicy, MAX_BLOCK_CACHE_SIZE, MAX_UNVERIFIED_CACHE_BYTES, MAX_UNVERIFIED_CACHE_SIZE,
    MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT, PersistedBodyCache, RetainedBlock,
};
pub use types::*;
