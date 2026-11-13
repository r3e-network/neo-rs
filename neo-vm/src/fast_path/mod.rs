//! Fast path execution optimization for common transaction patterns.
//!
//! This module implements a dual-engine architecture:
//! - **Fast Path Engine**: Optimized execution for recognized NEP-17 transfers
//! - **Full VM Engine**: Complete execution for complex smart contracts and native block scripts
//!
//! The fast path engine bypasses the VM interpreter only for supported transaction
//! patterns. OnPersist and PostPersist are classified here but always execute through
//! the canonical ApplicationEngine path. Performance claims must be established by
//! benchmarks rather than assumed from the dispatch architecture.
//!
//! ## Safety
//!
//! The fast path must produce **exactly** the same results as the full VM:
//! - Automatic fallback to full VM if fast path fails
//! - Optional verification mode (dual execution + result comparison)
//! - Detailed logging of all fast path decisions

pub mod block_scripts;
pub mod classifier;
pub mod error;
pub mod executor;
pub mod nep17;

pub use classifier::{TransactionClassifier, TransactionComplexity};
pub use error::{FastPathError, FastPathResult};
pub use executor::FastPathExecutor;
pub use nep17::{EntryInstruction, Nep17Parser, TransferParams};
