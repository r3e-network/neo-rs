//! Typed service interfaces for core Neo subsystems.
//!
//! This module defines canonical service traits and provides implementations
//! for the concrete types that live inside the core crate (`LedgerContext`,
//! `StateStore`, `MemoryPool`, etc).
//!
//! Note: Service traits were migrated from the `neo-services` crate and are
//! now defined locally in the `traits` module.

pub mod traits;

use crate::ledger::{MemoryPool, ledger_context::LedgerContext};
use crate::state_service::state_store::StateStore;
use parking_lot::Mutex;
use std::sync::Arc;

pub use traits::{
    LedgerService, MempoolService, PeerManagerService, RpcService, StateStoreService, SystemContext,
};

impl LedgerService for LedgerContext {
    fn current_height(&self) -> u32 {
        LedgerContext::current_height(self)
    }

    fn current_header_height(&self) -> u32 {
        LedgerContext::highest_header_index(self)
    }

    fn block_hash_at(&self, index: u32) -> Option<[u8; 32]> {
        LedgerContext::block_hash_at(self, index).map(|hash| hash.as_bytes())
    }
}

impl StateStoreService for StateStore {
    fn local_root_index(&self) -> Option<u32> {
        StateStore::local_root_index(self)
    }

    fn validated_root_index(&self) -> Option<u32> {
        StateStore::validated_root_index(self)
    }
}

impl MempoolService for MemoryPool {
    fn count(&self) -> usize {
        MemoryPool::count(self)
    }
}

/// Wrapper that exposes a [`MemoryPool`] protected by a mutex through the [`MempoolService`] trait.
#[derive(Clone)]
pub struct LockedMempoolService {
    inner: Arc<Mutex<MemoryPool>>,
}

impl LockedMempoolService {
    /// Creates a new locked mempool service wrapping a mutex-protected memory pool.
    ///
    /// This wrapper enables thread-safe access to mempool statistics through
    /// the [`MempoolService`] trait interface.
    ///
    /// # Arguments
    /// * `inner` - Arc-wrapped mutex protecting the underlying [`MemoryPool`]
    pub fn new(inner: Arc<Mutex<MemoryPool>>) -> Self {
        Self { inner }
    }
}

impl MempoolService for LockedMempoolService {
    fn count(&self) -> usize {
        self.inner.lock().count()
    }
}
