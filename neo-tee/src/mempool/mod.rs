//! TEE-protected mempool with fair transaction ordering
//!
//! This module implements a fair ordering policy to prevent MEV attacks.

// `fair_ordering` and `tee_mempool` are crate-visible (not just module-private)
// so the feature-gated `nitro` backend can reference `OrderingProof`,
// `FairOrderingPolicy` variants, and the sequencer types it reuses verbatim.
pub(crate) mod fair_ordering;
pub(crate) mod tee_mempool;

pub use fair_ordering::FairOrderingPolicy;
pub use tee_mempool::{TeeMempool, TeeMempoolConfig};
