//! Telemetry module for Neo node metrics and observability.
//!
//! **IMPORTANT**: This module provides **internal metrics collection** for neo-core.
//! For production deployment with Prometheus endpoints and system monitoring,
//! use the `neo_telemetry` crate instead.
//!
//! ## When to use this module
//!
//! - **Internal metrics**: Recording blockchain metrics within neo-core components
//! - **No external dependencies**: When you need lightweight metric collection
//! - **Snapshot export**: Getting point-in-time metric snapshots in JSON or Prometheus text
//! - **Timer utilities**: Measuring operation durations
//!
//! ## When to use neo-telemetry
//!
//! - **Production deployment**: HTTP metrics endpoint for Prometheus scraping
//! - **System monitoring**: CPU, memory, disk usage metrics
//! - **Health checks**: Liveness and readiness probes for Kubernetes
//! - **Logging configuration**: Structured logging setup
//!
//! This module supports:
//! - Metrics collection (counters, gauges, histograms)
//! - Structured event logging
//! - Performance profiling
//! - Health status reporting
//!
//! # Architecture
//!
//! The telemetry system is designed to be:
//! - **Lightweight**: Minimal overhead when metrics are not being collected
//! - **Extensible**: Easy to add new metrics and exporters
//! - **Compatible**: Works with existing `tracing` infrastructure
//!
//! # Example
//!
//! ```ignore
//! use neo_core::telemetry::{Telemetry, MetricType};
//!
//! let telemetry = Telemetry::new("neo-node", "1.0.0");
//! telemetry.record_gauge("block_height", 12345.0);
//! telemetry.increment_counter("transactions_processed");
//! ```

mod metrics;
mod recorder;

pub use metrics::{Counter, Gauge, Histogram, MetricValue};
pub use recorder::{MetricsRecorder, MetricsSnapshot};

use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, instrument, trace, warn};

/// Main telemetry interface for the Neo node.
#[derive(Clone)]
pub struct Telemetry {
    service_name: String,
    version: String,
    recorder: Arc<MetricsRecorder>,
    start_time: Instant,
}

impl Telemetry {
    /// Creates a new telemetry instance.
    pub fn new(service_name: impl Into<String>, version: impl Into<String>) -> Self {
        let service_name = service_name.into();
        let version = version.into();

        info!(
            target: "telemetry",
            service = %service_name,
            version = %version,
            "Telemetry initialized"
        );

        Self {
            service_name,
            version,
            recorder: Arc::new(MetricsRecorder::new()),
            start_time: Instant::now(),
        }
    }

    /// Returns the service name.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Returns the service version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns the uptime since telemetry was initialized.
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Records a gauge metric value.
    #[instrument(skip(self), level = "trace")]
    pub fn record_gauge(&self, name: &str, value: f64) {
        self.recorder.record_gauge(name, value);
        trace!(target: "telemetry", metric = name, value = value, "gauge recorded");
    }

    /// Increments a counter metric.
    #[instrument(skip(self), level = "trace")]
    pub fn increment_counter(&self, name: &str) {
        self.recorder.increment_counter(name, 1);
        trace!(target: "telemetry", metric = name, "counter incremented");
    }

    /// Increments a counter metric by a specific amount.
    #[instrument(skip(self), level = "trace")]
    pub fn increment_counter_by(&self, name: &str, amount: u64) {
        self.recorder.increment_counter(name, amount);
        trace!(target: "telemetry", metric = name, amount = amount, "counter incremented");
    }

    /// Records a histogram observation.
    #[instrument(skip(self), level = "trace")]
    pub fn record_histogram(&self, name: &str, value: f64) {
        self.recorder.record_histogram(name, value);
        trace!(target: "telemetry", metric = name, value = value, "histogram recorded");
    }

    /// Records a duration in milliseconds to a histogram.
    pub fn record_duration_ms(&self, name: &str, duration: Duration) {
        self.record_histogram(name, duration.as_secs_f64() * 1000.0);
    }

    /// Returns a snapshot of all current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.recorder.snapshot()
    }

    /// Returns the metrics recorder for direct access.
    pub fn recorder(&self) -> &Arc<MetricsRecorder> {
        &self.recorder
    }

    /// Creates a new timer for measuring operation duration.
    pub fn start_timer(&self, metric_name: &str) -> TelemetryTimer {
        TelemetryTimer::new(metric_name.to_string(), self.recorder.clone())
    }

    /// Records blockchain-specific metrics.
    pub fn record_blockchain_metrics(
        &self,
        block_height: u32,
        header_height: u32,
        mempool_size: u32,
        peer_count: usize,
    ) {
        self.record_gauge("neo_block_height", block_height as f64);
        self.record_gauge("neo_header_height", header_height as f64);
        self.record_gauge(
            "neo_header_lag",
            (header_height.saturating_sub(block_height)) as f64,
        );
        self.record_gauge("neo_mempool_size", mempool_size as f64);
        self.record_gauge("neo_peer_count", peer_count as f64);

        debug!(
            target: "telemetry",
            block_height,
            header_height,
            mempool_size,
            peer_count,
            "blockchain metrics updated"
        );
    }

    /// Records P2P network timeout statistics.
    pub fn record_timeout_stats(&self, handshake: u64, read: u64, write: u64) {
        self.record_gauge("neo_p2p_timeouts_handshake", handshake as f64);
        self.record_gauge("neo_p2p_timeouts_read", read as f64);
        self.record_gauge("neo_p2p_timeouts_write", write as f64);
    }

    /// Records state service metrics.
    pub fn record_state_metrics(
        &self,
        local_root_index: Option<u32>,
        validated_root_index: Option<u32>,
        accepted: u64,
        rejected: u64,
    ) {
        self.record_gauge(
            "neo_state_local_root_index",
            local_root_index.map(|v| v as f64).unwrap_or(-1.0),
        );
        self.record_gauge(
            "neo_state_validated_root_index",
            validated_root_index.map(|v| v as f64).unwrap_or(-1.0),
        );

        if let (Some(local), Some(validated)) = (local_root_index, validated_root_index) {
            self.record_gauge(
                "neo_state_validated_lag",
                local.saturating_sub(validated) as f64,
            );
        }

        self.record_gauge("neo_state_roots_accepted_total", accepted as f64);
        self.record_gauge("neo_state_roots_rejected_total", rejected as f64);
    }

    /// Records storage metrics.
    pub fn record_storage_metrics(&self, free_bytes: u64, total_bytes: u64) {
        self.record_gauge("neo_storage_free_bytes", free_bytes as f64);
        self.record_gauge("neo_storage_total_bytes", total_bytes as f64);
    }
}

/// Timer for measuring operation duration.
pub struct TelemetryTimer {
    metric_name: String,
    recorder: Arc<MetricsRecorder>,
    start: Instant,
}

impl TelemetryTimer {
    fn new(metric_name: String, recorder: Arc<MetricsRecorder>) -> Self {
        Self {
            metric_name,
            recorder,
            start: Instant::now(),
        }
    }

    /// Stops the timer and records the duration.
    pub fn stop(self) -> Duration {
        let duration = self.start.elapsed();
        self.recorder
            .record_histogram(&self.metric_name, duration.as_secs_f64() * 1000.0);
        duration
    }

    /// Returns the elapsed time without stopping the timer.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for TelemetryTimer {
    fn drop(&mut self) {
        // Record duration on drop if not explicitly stopped
        let duration = self.start.elapsed();
        self.recorder
            .record_histogram(&self.metric_name, duration.as_secs_f64() * 1000.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_records_gauges() {
        let telemetry = Telemetry::new("test-service", "1.0.0");
        telemetry.record_gauge("test_gauge", 42.0);

        let snapshot = telemetry.snapshot();
        assert!(snapshot.gauges.contains_key("test_gauge"));
        assert_eq!(snapshot.gauges.get("test_gauge"), Some(&42.0));
    }

    #[test]
    fn telemetry_increments_counters() {
        let telemetry = Telemetry::new("test-service", "1.0.0");
        telemetry.increment_counter("test_counter");
        telemetry.increment_counter("test_counter");
        telemetry.increment_counter_by("test_counter", 3);

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.counters.get("test_counter"), Some(&5));
    }

    #[test]
    fn telemetry_records_blockchain_metrics() {
        let telemetry = Telemetry::new("test-service", "1.0.0");
        telemetry.record_blockchain_metrics(100, 105, 50, 10);

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.gauges.get("neo_block_height"), Some(&100.0));
        assert_eq!(snapshot.gauges.get("neo_header_height"), Some(&105.0));
        assert_eq!(snapshot.gauges.get("neo_header_lag"), Some(&5.0));
        assert_eq!(snapshot.gauges.get("neo_mempool_size"), Some(&50.0));
        assert_eq!(snapshot.gauges.get("neo_peer_count"), Some(&10.0));
    }

    #[test]
    fn timer_records_duration() {
        let telemetry = Telemetry::new("test-service", "1.0.0");
        let timer = telemetry.start_timer("test_operation");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let duration = timer.stop();

        assert!(duration.as_millis() >= 10);

        let snapshot = telemetry.snapshot();
        assert!(snapshot.histograms.contains_key("test_operation"));
    }
}
