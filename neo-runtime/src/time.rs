//! Elapsed-time helpers that avoid silent u128→u64 truncation.
//!
//! The standard library's `Duration::as_micros()` returns `u128`, which
//! silently truncates when cast to `u64`. In practice this only matters for
//! durations exceeding ~584 million years, but the saturating conversion here
//! is strictly safer and documents the intent.

use std::time::Duration;

/// Returns the duration in microseconds, saturating on overflow.
///
/// # Example
///
/// ```rust,ignore
/// let start = std::time::Instant::now();
/// // ... do work ...
/// let us = neo_runtime::time::elapsed_us(start.elapsed());
/// ```
pub fn elapsed_us(duration: Duration) -> u64 {
    duration.as_micros().min(u64::MAX as u128) as u64
}

/// Returns the duration in milliseconds, saturating on overflow.
pub fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
