//! # neo-rpc::server::session
//!
//! RPC session records and connection-local state.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `dummy_block`: C#-compatible dummy persisting block construction.
//! - `execution`: Session construction and initial script execution.
//! - `iterators`: RPC session iterator retention and disposal helpers.
//! - `ledger_provider`: Ledger read seam used by dummy block construction.
//! - `native_provider`: Native-contract read seam used by session construction.
//! - `tests`: Module-local tests and regression coverage.

mod dummy_block;
mod execution;
mod iterators;
mod ledger_provider;
mod native_provider;

use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use neo_execution::ApplicationEngine;
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{CacheRead, DataCache, EmptyCacheBacking, StoreCacheBacking};
use std::sync::Arc;
use uuid::Uuid;

use crate::server::diagnostic::Diagnostic;

use iterators::IteratorEntry;

type SessionEngine<B> =
    ApplicationEngine<neo_native_contracts::StandardNativeProvider, Option<Diagnostic>, B>;

/// Invocation session backed by the runtime-selected persistent store.
pub type Session = SessionRecord<StoreCacheBacking<RuntimeStore>>;

/// Represents an invocation session that can retain iterators between RPC calls.
pub struct SessionRecord<B = EmptyCacheBacking>
where
    B: CacheRead,
{
    script: Vec<u8>,
    snapshot: Arc<DataCache<B>>,
    engine: Mutex<SessionEngine<B>>,
    diagnostic: Mutex<Option<Diagnostic>>,
    iterators: Mutex<HashMap<Uuid, IteratorEntry>>,
    iterator_lookup: Mutex<HashMap<u32, Uuid>>,
    start_time: Mutex<Instant>,
}

impl<B> SessionRecord<B>
where
    B: CacheRead,
{
    /// Return the script executed by this session.
    pub fn script(&self) -> &[u8] {
        &self.script
    }

    /// Lock and return the session's application engine.
    pub fn engine(&self) -> MutexGuard<'_, SessionEngine<B>> {
        self.engine.lock()
    }

    /// Lock and return the session's application engine for mutable use.
    pub fn engine_mut(&self) -> MutexGuard<'_, SessionEngine<B>> {
        self.engine()
    }

    /// Return a clone of the diagnostic information captured during execution.
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        self.diagnostic.lock().clone()
    }

    /// Return the storage snapshot associated with this session.
    pub fn snapshot(&self) -> &DataCache<B> {
        &self.snapshot
    }

    /// Reset the session expiration timer to the current instant.
    pub fn reset_expiration(&self) {
        let mut start_time = self.start_time.lock();
        *start_time = Instant::now();
    }

    /// Return whether the session has lived for at least `expiration`.
    pub fn is_expired(&self, expiration: Duration) -> bool {
        self.start_time.lock().elapsed() >= expiration
    }
}

// THREAD SAFETY
//
// `ApplicationEngine` (and the underlying `ExecutionEngine`) is now `Send`
// because `HostPtr` implements `Send + Sync` with its safety invariants
// enforced at construction time. All mutable state in `Session` is guarded
// by `parking_lot::Mutex`, so `Session` is naturally `Send + Sync` without
// manual unsafe impls.

#[cfg(test)]
#[path = "../../tests/server/core/session.rs"]
mod tests;
