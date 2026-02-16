//! Metric types for the telemetry system.

use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// A counter metric that can only increase.
#[derive(Debug, Default)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Creates a new counter with initial value 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Increments the counter by 1.
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the counter by the given amount.
    pub fn increment_by(&self, amount: u64) {
        self.value.fetch_add(amount, Ordering::Relaxed);
    }

    /// Returns the current value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// A gauge metric that can increase or decrease.
#[derive(Debug, Default)]
pub struct Gauge {
    // Store as bits for atomic operations on f64
    value: AtomicU64,
}

impl Gauge {
    /// Creates a new gauge with initial value 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the gauge to the given value.
    pub fn set(&self, value: f64) {
        self.value.store(value.to_bits(), Ordering::Relaxed);
    }

    /// Returns the current value.
    pub fn get(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    /// Increments the gauge by the given amount.
    pub fn increment(&self, amount: f64) {
        loop {
            let current = self.value.load(Ordering::Relaxed);
            let current_f64 = f64::from_bits(current);
            let new_value = (current_f64 + amount).to_bits();
            if self
                .value
                .compare_exchange_weak(current, new_value, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Decrements the gauge by the given amount.
    pub fn decrement(&self, amount: f64) {
        self.increment(-amount);
    }
}

/// A histogram metric for recording distributions.
#[derive(Debug)]
pub struct Histogram {
    count: AtomicU64,
    sum: AtomicU64,                   // Stored as f64 bits
    min: AtomicU64,                   // Stored as f64 bits
    max: AtomicU64,                   // Stored as f64 bits
    buckets: RwLock<Vec<(f64, u64)>>, // (upper_bound, count)
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Histogram {
    /// Creates a new histogram with default buckets.
    pub fn new() -> Self {
        Self::with_buckets(vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ])
    }

    /// Creates a new histogram with custom bucket boundaries.
    pub fn with_buckets(boundaries: Vec<f64>) -> Self {
        let buckets = boundaries.into_iter().map(|b| (b, 0u64)).collect();
        Self {
            count: AtomicU64::new(0),
            sum: AtomicU64::new(0.0f64.to_bits()),
            min: AtomicU64::new(f64::MAX.to_bits()),
            max: AtomicU64::new(f64::MIN.to_bits()),
            buckets: RwLock::new(buckets),
        }
    }

    /// Records an observation.
    pub fn observe(&self, value: f64) {
        self.count.fetch_add(1, Ordering::Relaxed);

        // Update sum
        loop {
            let current = self.sum.load(Ordering::Relaxed);
            let current_f64 = f64::from_bits(current);
            let new_value = (current_f64 + value).to_bits();
            if self
                .sum
                .compare_exchange_weak(current, new_value, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        // Update min
        loop {
            let current = self.min.load(Ordering::Relaxed);
            let current_f64 = f64::from_bits(current);
            if value >= current_f64 {
                break;
            }
            if self
                .min
                .compare_exchange_weak(
                    current,
                    value.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }

        // Update max
        loop {
            let current = self.max.load(Ordering::Relaxed);
            let current_f64 = f64::from_bits(current);
            if value <= current_f64 {
                break;
            }
            if self
                .max
                .compare_exchange_weak(
                    current,
                    value.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }

        // Update buckets
        let mut buckets = self.buckets.write();
        for (bound, count) in buckets.iter_mut() {
            if value <= *bound {
                *count += 1;
            }
        }
    }

    /// Returns the count of observations.
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Returns the sum of all observations.
    pub fn sum(&self) -> f64 {
        f64::from_bits(self.sum.load(Ordering::Relaxed))
    }

    /// Returns the minimum observed value.
    pub fn min(&self) -> f64 {
        let min = f64::from_bits(self.min.load(Ordering::Relaxed));
        if min == f64::MAX { 0.0 } else { min }
    }

    /// Returns the maximum observed value.
    pub fn max(&self) -> f64 {
        let max = f64::from_bits(self.max.load(Ordering::Relaxed));
        if max == f64::MIN { 0.0 } else { max }
    }

    /// Returns the average of all observations.
    pub fn avg(&self) -> f64 {
        let count = self.count();
        if count == 0 {
            0.0
        } else {
            self.sum() / count as f64
        }
    }
}

/// Enum representing different metric values.
#[derive(Debug, Clone)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    Histogram {
        count: u64,
        sum: f64,
        min: f64,
        max: f64,
        avg: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_increments() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);

        counter.increment();
        assert_eq!(counter.get(), 1);

        counter.increment_by(5);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn gauge_sets_and_increments() {
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0.0);

        gauge.set(42.0);
        assert_eq!(gauge.get(), 42.0);

        gauge.increment(8.0);
        assert_eq!(gauge.get(), 50.0);

        gauge.decrement(10.0);
        assert_eq!(gauge.get(), 40.0);
    }

    #[test]
    fn histogram_records_observations() {
        let histogram = Histogram::new();

        histogram.observe(0.5);
        histogram.observe(1.5);
        histogram.observe(2.5);

        assert_eq!(histogram.count(), 3);
        assert!((histogram.sum() - 4.5).abs() < 0.001);
        assert!((histogram.min() - 0.5).abs() < 0.001);
        assert!((histogram.max() - 2.5).abs() < 0.001);
        assert!((histogram.avg() - 1.5).abs() < 0.001);
    }
}
