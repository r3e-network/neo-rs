//! Service trait definitions for the mempool subsystem.
//!
//! Distributed from `neo-core::services::traits` and `neo-core::services::mod`.

use super::memory_pool::MemoryPool;
use parking_lot::{RwLock, Mutex};
use std::sync::Arc;

/// Mempool operations used across networking and RPC layers.
///
/// Provides basic statistics about the transaction pool.
pub trait MempoolService: Send + Sync {
    /// Total transaction count tracked by the pool.
    fn count(&self) -> usize;
}

/// Wrapper that exposes a [`MemoryPool`] protected by a RwLock through the [`MempoolService`] trait.
#[derive(Clone)]
pub struct LockedMempoolService {
    inner: Arc<RwLock<MemoryPool>>,
}

impl LockedMempoolService {
    /// Creates a new locked mempool service wrapping a RwLock-protected memory pool.
    ///
    /// This wrapper enables thread-safe access to mempool statistics through
    /// the [`MempoolService`] trait interface.
    ///
    /// # Arguments
    /// * `inner` - Arc-wrapped RwLock protecting the underlying [`MemoryPool`]
    pub fn new(inner: Arc<RwLock<MemoryPool>>) -> Self {
        Self { inner }
    }
}

/// Wrapper that exposes a [`MemoryPool`] protected by a Mutex through the [`MempoolService`] trait.
#[derive(Clone)]
pub struct MutexMempoolService {
    inner: Arc<Mutex<MemoryPool>>,
}

impl MutexMempoolService {
    /// Creates a new locked mempool service wrapping a Mutex-protected memory pool.
    ///
    /// This wrapper enables thread-safe access to mempool statistics through
    /// the [`MempoolService`] trait interface.
    ///
    /// # Arguments
    /// * `inner` - Arc-wrapped Mutex protecting the underlying [`MemoryPool`]
    pub fn new(inner: Arc<Mutex<MemoryPool>>) -> Self {
        Self { inner }
    }
}

impl MempoolService for LockedMempoolService {
    fn count(&self) -> usize {
        self.inner.read().count()
    }
}

impl MempoolService for MutexMempoolService {
    fn count(&self) -> usize {
        self.inner.lock().count()
    }
}

impl MempoolService for MemoryPool {
    fn count(&self) -> usize {
        MemoryPool::count(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockMempool {
        count: usize,
    }

    impl MempoolService for MockMempool {
        fn count(&self) -> usize {
            self.count
        }
    }

    #[test]
    fn mock_mempool_service_works() {
        let mempool = MockMempool { count: 42 };
        assert_eq!(mempool.count(), 42);
    }

    #[test]
    fn trait_is_object_safe() {
        fn _accepts_mempool(_: &dyn MempoolService) {}
    }
}
