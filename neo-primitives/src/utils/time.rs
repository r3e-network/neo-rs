//! Testable time source for the Neo workspace.
//!
//! This module owns [`TimeProvider`] and [`TimeSource`]. Other crates depend on
//! it instead of calling `Utc::now()` directly, so tests can override the clock
//! with a fixed timestamp and avoid flaky time-dependent assertions.
//!
//! ## Layering
//!
//! Sits in `neo-primitives` with the other foundational, no-`neo-*`
//! dependency types. It depends only on `chrono` and `parking_lot`.
//!
//! ## Why a foundation primitive
//!
//! `TimeProvider` is a process-wide testable time source. Folding it into
//! `neo-system` would force every crate that wants a mockable clock
//! (consensus, validation, telemetry, transaction pool) to depend on the
//! system orchestrator and pull in P2P, storage, state, and execution.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use neo_primitives::{TimeProvider, TimeSource};
//!
//! // Get current time.
//! let now = TimeProvider::current().utc_now();
//!
//! // For testing, override with a fixed timestamp.
//! TimeProvider::set_current(TimeSource::fixed_millis(1_600_000_000_000));
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeZone, Utc};
use parking_lot::RwLock;
use std::sync::LazyLock;

/// Returns the current time in milliseconds since the Unix epoch.
///
/// Returns 0 if the system clock is before the epoch (should never happen
/// in practice but avoids panicking on misconfigured clocks).
///
/// This is the canonical epoch-millis helper shared across the workspace.
/// Consensus view-timeouts, state-root retry backoff, and block timestamp
/// derivation all use this clock.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Concrete process clock used by [`TimeProvider`].
///
/// This is intentionally a closed enum instead of a trait object. The node only
/// needs the production system clock and deterministic fixed clocks for tests;
/// keeping that surface explicit avoids trait-object dispatch in consensus and
/// validation paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSource {
    /// Use the operating system clock.
    System,
    /// Always return the stored Unix timestamp in milliseconds.
    FixedMillis(i64),
}

impl TimeSource {
    /// Creates the default system time source.
    #[must_use]
    pub const fn system() -> Self {
        Self::System
    }

    /// Creates a deterministic fixed time source.
    #[must_use]
    pub const fn fixed_millis(timestamp_millis: i64) -> Self {
        Self::FixedMillis(timestamp_millis)
    }

    /// Returns the current UTC time.
    #[must_use]
    pub fn utc_now(&self) -> DateTime<Utc> {
        match self {
            Self::System => Utc::now(),
            Self::FixedMillis(millis) => Utc
                .timestamp_millis_opt(*millis)
                .single()
                .expect("fixed timestamp is representable"),
        }
    }

    /// Returns the current UTC time as milliseconds since Unix epoch.
    #[must_use]
    pub fn utc_now_timestamp_millis(&self) -> i64 {
        datetime_to_millis(self.utc_now())
    }
}

/// Global holder for the currently active time source.
static CURRENT_TIME_SOURCE: LazyLock<RwLock<TimeSource>> =
    LazyLock::new(|| RwLock::new(TimeSource::system()));

/// Time provider facade replicating the behaviour of the C# implementation.
#[derive(Debug, Clone, Copy)]
pub struct TimeProvider;

impl TimeProvider {
    /// Returns the currently configured time source.
    pub fn current() -> TimeSource {
        *CURRENT_TIME_SOURCE.read()
    }

    /// Overrides the currently configured time source.
    pub fn set_current(source: TimeSource) {
        *CURRENT_TIME_SOURCE.write() = source;
    }

    /// Resets the time source back to the default system implementation.
    pub fn reset_to_default() {
        *CURRENT_TIME_SOURCE.write() = TimeSource::system();
    }
}

fn datetime_to_millis(datetime: DateTime<Utc>) -> i64 {
    datetime.timestamp_millis()
}

#[cfg(test)]
#[path = "../tests/utils/time.rs"]
mod tests;
