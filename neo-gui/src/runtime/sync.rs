//! Shared synchronization helpers for GUI state.
//!
//! The GUI keeps short-lived state snapshots behind standard-library mutexes.
//! This module centralizes poison handling so screens and worker threads do not
//! choose different panic/recovery policies.

use std::sync::{Mutex, MutexGuard};

/// Lock a GUI mutex and recover the inner value if a prior holder panicked.
pub(crate) fn lock<'a, T>(mutex: &'a Mutex<T>, name: &'static str) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target: "neo_gui",
                mutex = name,
                "recovering poisoned GUI mutex"
            );
            poisoned.into_inner()
        }
    }
}

#[cfg(test)]
#[path = "tests/sync.rs"]
mod tests;
