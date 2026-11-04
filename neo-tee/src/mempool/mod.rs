//! TEE-protected mempool with fair transaction ordering
//!
//! This module implements a fair ordering policy to prevent MEV attacks.
//!
//! ## Privacy Mode Phase A: encrypted contents (strictly opt-in)
//!
//! - **crypto**: content-encryption engine — AEAD payload sealing plus
//!   KEM-style data-key encapsulation (simulated committee epoch key, or the
//!   rollback-floor-guarded enclave sealing key)
//! - **batcher**: Contract-based transaction batching (Solana-inspired)
//! - **fair_ordering**: Fair transaction ordering for MEV prevention
//! - **tee_mempool**: Core TEE mempool implementation with the
//!   `Encrypted -> Resolved | Rejected` decrypt-stage state machine
//!
//! Simulation caveat: under the `simulation` feature the encrypted mempool
//! is interface and flow verification only — host-side encryption provides
//! no real enclave isolation guarantees. Production requires a real TEE
//! path. Phase A covers the "encrypted intake -> TEE decrypt -> ordered hash
//! table" link only.

mod batcher;
mod crypto;
mod fair_ordering;
mod tee_mempool;

pub use batcher::{ContractBatchScheduler, ExecBatch, SchedulingMetrics};
pub use crypto::{
    CIPHERTEXT_VERSION, CommitteeEpochKey, ContentCipher, KeyEncapsulation, MODE_COMMITTEE_KEY,
    MODE_TEE_SEALED_KEY, MempoolCiphertext, TEE_SEALED_EPOCH_ID,
};
pub use fair_ordering::FairOrderingPolicy;
pub use tee_mempool::{
    ContentEncryptionConfig, ContentState, EncryptionMode, ResolveSummary, TeeMempool,
    TeeMempoolConfig,
};
