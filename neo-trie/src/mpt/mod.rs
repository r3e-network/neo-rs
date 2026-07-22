//! # Neo MPT Implementation
//!
//! ## Boundary
//!
//! This module owns the internal Neo-compatible node graph, mutation cache,
//! proof, traversal, and persisted-root validation implementation. Durable
//! storage policy and StateService lifecycle remain outside this crate.
//!
//! ## Contents
//!
//! Cache/store capabilities, typed errors, node codecs, validation, metrics,
//! and trie operations. The crate root re-exports the stable public surface.

pub(crate) mod cache;
pub(crate) mod error;
pub(crate) mod metrics;
pub(crate) mod node;
pub(crate) mod node_type;
pub(crate) mod root_validation;
pub(crate) mod trie;

pub use cache::{
    MptCache, MptMutationStats, MptStoreLookup, MptStoreSnapshot, UnresolvedDeferredNode,
};
pub use error::{MptError, MptResult};
pub use node::Node;
pub use node_type::NodeType;
pub use root_validation::{
    PersistedMptGraphLimits, PersistedMptGraphReport, validate_persisted_root_graph,
};
pub use trie::{Trie, TrieEntry};

/// Prefix of every content-addressed MPT node key in Neo state storage.
pub const MPT_NODE_PREFIX: u8 = 0xf0;
