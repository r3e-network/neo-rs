//! # neo-state-service::providers
//!
//! Frozen state-read capabilities and factories over the persisted StateService
//! MPT.
//!
//! ## Boundary
//!
//! This module hides snapshots, trie construction, and mutable trie-resolution
//! caches from RPC and other application layers. Consumers select a root or
//! height through [`StateProviderFactory`] and then use the returned concrete
//! [`StateView`]. Provider implementations must preserve the existing Neo MPT
//! key/value bytes and point-in-time read isolation.
//!
//! ## Contents
//!
//! - `error`: Provider-level error vocabulary.
//! - `mpt`: Statically dispatched provider and factory over [`crate::MptStore`].
//! - `proof`: State-proof verification without exposing trie implementation
//!   types.
//! - `traits`: Capability-oriented state view and factory contracts.

mod error;
mod mpt;
mod proof;
mod traits;

pub use error::{StateProviderError, StateProviderResult};
pub use mpt::{MptStateProvider, MptStateProviderFactory};
pub use proof::verify_state_proof;
pub use traits::{StateEntry, StateProof, StateProviderFactory, StateView};
