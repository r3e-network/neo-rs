//! Tests for the error-report retry backoff schedule.

use std::time::Duration;

use super::super::retry_backoff;

#[test]
fn retry_backoff_is_exponential_with_a_ceiling() {
    let base = 250;
    // attempt 0 is the first try (no wait); retries start at attempt 1.
    assert_eq!(retry_backoff(base, 1), Duration::from_millis(250));
    assert_eq!(retry_backoff(base, 2), Duration::from_millis(500));
    assert_eq!(retry_backoff(base, 3), Duration::from_millis(1_000));
    // The exponent is capped, and the total delay never exceeds 30s.
    assert_eq!(retry_backoff(base, 50), Duration::from_millis(16_000));
    assert_eq!(retry_backoff(1_000_000, 5), Duration::from_millis(30_000));
}

#[test]
fn retry_backoff_treats_zero_base_as_one_millisecond() {
    assert_eq!(retry_backoff(0, 1), Duration::from_millis(1));
}
