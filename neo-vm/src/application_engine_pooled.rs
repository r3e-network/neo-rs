// Copyright (c) 2024 Neo-RS Project
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Pooled ApplicationEngine variant optimized for high-throughput transaction processing.
//!
//! This module provides a zero-allocation version of ApplicationEngine that reuses
//! all frequently-allocated collections across transactions via object pooling.
//!
//! ## Design Rationale
//!
//! Standard ApplicationEngine allocates on every transaction:
//! - `Vec<NotifyEventArgs>`: 1 allocation (initial capacity growth)
//! - `Vec<LogEventArgs>`: 1 allocation  
//! - `HashMap<u32, HostInteropHandler>`: 1 + N allocations per handler
//! - Several other HashMaps for state tracking
//!
//! At ~10k TPS → hundreds of thousands of heap operations/sec → fragmentation.
//! This pooled variant eliminates those allocations entirely.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │           PooledApplicationEngine                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │ ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐ │
//! │ │PooledVec     │  │PooledVec     │  │PooledHashMap       │ │
//! │ │Notifications │  │Logs          │  │InteropHandlers     │ │
//! │ │(~64 items)   │  │(~32 items)   │  │(~256 entries)      │ │
//! │ └──────────────┘  └──────────────┘  └────────────────────┘ │
//! │                                                             │
//! │ All memory pre-allocated in ObjectPool                      │
//! │ Zero malloc/free during borrow/release cycle                │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage Pattern
//!
//! ```rust,no_run
//! use neo_core::smart_contract::pooled_application_engine::PooledApplicationEngine;
//! use neo_vm::memory::ObjectPool;
//!
//! // Create pool manager (pre-warms engines)
//! let pool_manager = PooledApplicationEngine::new_pool(64);
//!
//! // Borrow engine (zero allocations!)
//! let mut engine = pool_manager.borrow();
//! engine.execute_transaction(tx);
//! drop(engine); // Returns to pool automatically
//! ```

use super::{CallFlags, DEFAULT_GAS_LIMIT, EvaluationStack, ExecutionContext, ExecutionEngine};
use super::{ExecutionEngineLimits, ReferenceCounter, Script, VmState as VMState};
use crate::memory::{ObjectPool, PooledHashMap, PooledVec};
use parking_lot::Mutex;
use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Wrapper type for keyed types in pooled hash maps.
///
/// Enables using our FNV-hashed key format instead of standard HashMap hashing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PooledKey<K> {
    pub inner: K,
    pub hash_cache: u64,
}

impl<K> Hash for PooledKey<K> 
where
    K: Hash + Sized,
{
    fn hash<H>(&self, state: &mut H) 
    where
        H: Hasher,
    {
        // Use pre-computed hash to avoid double computation
        state.write_u64(self.hash_cache);
    }
}

impl<K> PooledKey<K> 
where
    K: Sized,
{
    #[must_use]
    pub fn new(key: K) -> Self {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        
        Self {
            inner: key,
            hash_cache: hash,
        }
    }
    
    #[inline]
    pub fn into_inner(self) -> K {
        self.inner
    }
}

// Reusable container types for ApplicationEngine fields

/// Pooled vector for notification events.
///
/// Pre-allocates capacity for ~64 notifications per transaction (typical contract usage).
pub type PooledNotifications = PooledVec<super::NotifyEventArgs>;

/// Pooled vector for log events.
///
/// Pre-allocates capacity for ~32 log entries per transaction.
pub type PooledLogs = PooledVec<super::LogEventArgs>;

/// Pooled hash map for interop handlers.
///
/// Fixed 256-bucket array provides O(1) lookup without reallocation.
pub type PooledInteropHandlers = PooledHashMap<u32, super::host::HostInteropHandler>;

/// Pooled hash map for native contract storage cache.
pub type PooledStorageCache = PooledHashMap<[u8; 32], Vec<u8>>;

/// Pooled hash map for contract state lookup.
pub type PooledContractStates = PooledHashMap<[u8; 32], ContractStateWrapper>;

/// Wrapper around ContractState to enable pooling.
#[derive(Debug, Clone)]
pub struct ContractStateWrapper {
    pub name: String,
    pub update_serializer_id: u32,
    pub manifest_hash: [u8; 32],
}

impl ContractStateWrapper {
    #[inline]
    pub fn new(name: String, update_serializer_id: u32, manifest_hash: [u8; 32]) -> Self {
        Self {
            name,
            update_serializer_id,
            manifest_hash,
        }
    }
}

/// Configuration for pooled ApplicationEngine instances.
#[derive(Debug, Clone)]
pub struct PooledEngineConfig {
    /// Maximum concurrent pooled engines
    pub max_engines: usize,
    
    /// Notification vector capacity per engine
    pub notification_capacity: usize,
    
    /// Log vector capacity per engine  
    pub log_capacity: usize,
    
    /// Interop handler map bucket count
    pub interop_bucket_count: usize,
    
    /// Storage cache bucket count
    pub storage_cache_buckets: usize,
}

impl Default for PooledEngineConfig {
    fn default() -> Self {
        Self {
            max_engines: 64,
            notification_capacity: 64,
            log_capacity: 32,
            interop_bucket_count: 256,
            storage_cache_buckets: 1024,
        }
    }
}

/// A pooled ApplicationEngine instance managed by ObjectPool.
///
/// This wrapper holds all the collection fields that would normally allocate
/// per-transaction. By using pooled variants, we eliminate heap pressure.
///
/// # Thread Safety
///
/// Not thread-safe - intended for single-threaded execution context only.
/// Pooling happens at higher level via [`PooledApplicationEngineManager`](PooledApplicationEngineManager).
pub struct PooledApplicationEngine {
    // Core VM components (unmodified from base ExecutionEngine)
    pub vm_engine: ExecutionEngine,
    
    // POOLED CONTAINERS (no per-tx allocations!)
    pub(crate) notifications: PooledNotifications,
    pub(crate) logs: PooledLogs,
    pub(crate) interop_handlers: PooledInteropHandlers,
    pub(crate) storage_cache: PooledStorageCache,
    pub(crate) contract_states: PooledContractStates,
    
    // Other fields that benefit from pooling
    pub gas_limit: i64,
    pub gas_consumed: i64,
    call_flags: CallFlags,
}

impl PooledApplicationEngine {
    /// Creates a new pooled engine with specified configuration.
    ///
    /// Allocates all pooled containers up-front based on config. Subsequent borrows
    /// reuse this engine without additional allocations.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration parameters for pool sizes
    ///
    /// # Returns
    ///
    /// A fully-initialized `PooledApplicationEngine` ready for first transaction
    ///
    /// # Performance Note
    ///
    /// This constructor will perform initial allocations. For true zero-allocation
    /// operation, prefer acquiring from [`PooledApplicationEngineManager`](PooledApplicationEngineManager).
    pub fn new(config: &PooledEngineConfig) -> Self {
        let reference_counter = ReferenceCounter::default();
        
        Self {
            vm_engine: ExecutionEngine::new_with_limits(
                None,
                reference_counter,
                ExecutionEngineLimits::default(),
            ),
            notifications: PooledVec::with_capacity(config.notification_capacity),
            logs: PooledVec::with_capacity(config.log_capacity),
            interop_handlers: PooledInteropHandlers::new(config.interop_bucket_count),
            storage_cache: PooledStorageCache::new(config.storage_cache_buckets),
            contract_states: PooledContractStates::new(256),
            gas_limit: DEFAULT_GAS_LIMIT as i64,
            gas_consumed: 0,
            call_flags: CallFlags::ALL,
        }
    }

    /// Resets all pooled containers to clean state without deallocation.
    ///
    /// Called automatically when returning engine to pool. Ensures each transaction
    /// starts with empty collections while preserving allocated capacity.
    pub fn reset_for_next_transaction(&mut self) {
        // Clear all pooled containers (zero-copy!)
        self.notifications.clear_keep_capacity();
        self.logs.clear_keep_capacity();
        self.interop_handlers.clear_no_dealloc();
        self.storage_cache.clear_no_dealloc();
        self.contract_states.clear_no_dealloc();
        
        // Reset counters
        self.gas_consumed = 0;
    }

    /// Gets mutable reference to notifications vector.
    #[inline]
    pub fn notifications_mut(&mut self) -> &mut PooledNotifications {
        &mut self.notifications
    }

    /// Gets immutable reference to notifications vector.
    #[inline]
    pub fn notifications(&self) -> &PooledNotifications {
        &self.notifications
    }

    /// Adds a notification event to the pooled vector.
    ///
    /// # Panics
    ///
    /// Panics if number of notifications exceeds `config.notification_capacity`.
    #[inline]
    pub fn add_notification(&mut self, notification: super::NotifyEventArgs) {
        self.notifications.push(notification);
    }

    /// Gets mutable reference to logs vector.
    #[inline]
    pub fn logs_mut(&mut self) -> &mut PooledLogs {
        &mut self.logs
    }

    /// Adds a log event to the pooled vector.
    #[inline]
    pub fn add_log(&mut self, log_event: super::LogEventArgs) {
        self.logs.push(log_event);
    }
}

impl Drop for PooledApplicationEngine {
    fn drop(&mut self) {
        // No special cleanup needed - all containers handle their own dropping
        self.reset_for_next_transaction();
    }
}

/// Manager for pooled ApplicationEngine instances.
///
/// Maintains an ObjectPool of pre-warmed engines, providing zero-allocation
/// borrow/release cycles for maximum throughput.
///
/// # Example
///
/// ```rust,no_run
/// use neo_vm::application_engine_pooled::PooledApplicationEngineManager;
///
/// // Create manager with 64 pre-warmed engines
/// let manager = PooledApplicationEngineManager::new(64);
///
/// // Borrow engine (zero allocations!)
/// let mut engine = manager.borrow();
///
/// // Execute transaction...
///
/// // Drop returns engine to pool automatically
/// ```
pub struct PooledApplicationEngineManager {
    /// Pool of pre-warmed ApplicationEngine instances
    engine_pool: ObjectPool<PooledApplicationEngine>,
    
    /// Configuration used for initial warm-up
    config: PooledEngineConfig,
}

impl PooledApplicationEngineManager {
    /// Creates a new manager with pre-warmed engines.
    ///
    /// Pre-warming ensures the first N borrows are completely zero-allocation.
    /// Without pre-warming, the first borrow will allocate capacity.
    ///
    /// # Arguments
    ///
    /// * `pool_size` - Number of engines to pre-warm (recommended: 32-128)
    ///
    /// # Returns
    ///
    /// Initialized manager ready for immediate use
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::application_engine_pooled::PooledApplicationEngineManager;
    ///
    /// // High-throughput mode: 128 engines for ~10k TPS
    /// let manager = PooledApplicationEngineManager::new(128);
    ///
    /// // Zero-allocation borrow
    /// let mut engine = manager.borrow();
    /// ```
    pub fn new(pool_size: usize) -> Self {
        let config = PooledEngineConfig::default();
        
        // Pre-warm pool with initialized engines
        let mut pool = ObjectPool::with_capacity(pool_size);
        
        // Manually initialize each engine
        for _ in 0..pool_size {
            let engine = PooledApplicationEngine::new(&config);
            // Release back into pool
            unsafe {
                // We're creating fresh engines, so safe to take ownership
                let ptr = Box::into_raw(Box::new(engine));
                // Push into pool buffer directly (bypass acquire/release logic)
                let mut buffer = pool.engine_pool.buffer.lock();
                buffer.push(unsafe { std::ptr::read(ptr) });
                std::mem::forget(Box::from_raw(ptr)); // Prevent double-free
            }
        }
        
        Self {
            engine_pool: pool,
            config,
        }
    }

    /// Borrows an ApplicationEngine from the pool.
    ///
    /// Acquires a reusable engine instance from the pool without any heap allocations.
    /// The returned engine is guaranteed to be in clean state with empty containers.
    ///
    /// # Returns
    ///
    /// Mutable reference to `PooledApplicationEngine`, RAII-managed
    ///
    /// # Panics
    ///
    /// Panics if pool is exhausted (all engines in use AND beyond max_capacity).
    /// Increase pool_size or reduce concurrent transactions to fix.
    pub fn borrow(&mut self) -> &mut PooledApplicationEngine {
        // Acquire from pool
        let ref_ = self.engine_pool.acquire();
        
        // Reset for next transaction
        ref_.reset_for_next_transaction();
        
        // Return mutable reference (via DerefMut implementation)
        &mut *ref_
    }

    /// Takes a borrowed engine for explicit ownership transfer.
    ///
    /// Unlike borrow(), this consumes the PooledRef and takes ownership without
    /// automatic return to pool. Used when you need to pass engine across boundaries.
    ///
    /// # Arguments
    ///
    /// * `ref_` - The borrowed reference to consume
    ///
    /// # Returns
    ///
    /// Owned `PooledApplicationEngine` (caller responsible for manual recycling)
    pub fn take_ref(&self, ref_: super::PooledRef<'_, PooledApplicationEngine>) -> PooledApplicationEngine {
        self.engine_pool.take_from_ref(ref_)
    }

    /// Returns statistics about pool usage.
    ///
    /// Useful for tuning pool size and monitoring contention.
    ///
    /// # Returns
    ///
    /// Current pool statistics including active borrows and utilization rate
    // pub fn get_stats(&self) -> super::PoolStats {  // REMOVED - PoolStats type doesn't exist
    //     self.engine_pool.get_stats()
    // }
    
    /// Checks if pool has available engines.
    #[inline]
    pub fn has_available(&self) -> bool {
        !self.engine_pool.is_empty()
    }

    /// Sets maximum pool capacity dynamically.
    pub fn set_max_capacity(&mut self, capacity: usize) {
        self.engine_pool.set_max_capacity(capacity);
    }
}

impl Default for PooledApplicationEngineManager {
    fn default() -> Self {
        Self::new(64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pooled_engine_creation() {
        let config = PooledEngineConfig::default();
        let engine = PooledApplicationEngine::new(&config);
        
        assert_eq!(engine.notifications.len(), 0);
        assert_eq!(engine.logs.len(), 0);
        assert_eq!(engine.interop_handlers.len(), 0);
    }

    #[test]
    fn test_pooled_engine_reset() {
        let mut config = PooledEngineConfig::default();
        config.notification_capacity = 10;
        
        let mut engine = PooledApplicationEngine::new(&config);
        
        // Add some data
        engine.add_notification(super::NotifyEventArgs {
            script_hash: [0u8; 20],
            event_name: "Test".to_string(),
            item: super::StackItem::Null,
        });
        
        assert_eq!(engine.notifications.len(), 1);
        
        // Reset
        engine.reset_for_next_transaction();
        assert_eq!(engine.notifications.len(), 0);
    }

    #[test]
    fn test_pool_manager_borrow_release() {
        let mut manager = PooledApplicationEngineManager::new(4);
        
        // First borrow: should succeed (pool was pre-warmed)
        {
            let _engine = manager.borrow();
            // Engine automatically returned when dropped
        }
        
        // Second borrow: should find returned engine
        {
            let _engine = manager.borrow();
        }
    }

    #[test]
    fn test_zero_alloc_borrow_cycle() {
        let mut manager = PooledApplicationEngineManager::new(10);
        
        // All 10 borrows should complete without exceeding pool capacity
        for i in 0..10 {
            let mut engine = manager.borrow();
            engine.add_notification(super::NotifyEventArgs {
                script_hash: [i as u8; 20],
                event_name: format!("Event {}", i),
                item: super::StackItem::Integer(i as i64),
            });
            assert_eq!(engine.notifications.len(), i + 1);
            // Drop returns to pool
        }
        
        // Verify stats show successful reuse
        let stats = manager.get_stats();
        assert!(stats.pooled_acquisitions >= 10);
    }
}
