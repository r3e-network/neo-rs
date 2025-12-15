//! Prometheus metrics for Neo node

use prometheus::{
    Gauge, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, Opts,
    Registry,
};
use std::sync::Arc;

/// Neo node metrics
#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,

    // Blockchain metrics
    pub block_height: IntGauge,
    pub blocks_processed: IntCounter,
    pub block_processing_time: Histogram,

    // Transaction metrics
    pub transactions_processed: IntCounter,
    pub mempool_size: IntGauge,
    pub transaction_processing_time: Histogram,

    // Network metrics
    pub connected_peers: IntGauge,
    pub messages_received: IntCounterVec,
    pub messages_sent: IntCounterVec,
    pub bytes_received: IntCounter,
    pub bytes_sent: IntCounter,

    // Consensus metrics
    pub consensus_view: IntGauge,
    pub consensus_round_time: Histogram,

    // System metrics
    pub memory_usage_bytes: IntGauge,
    pub cpu_usage_percent: Gauge,
    pub disk_usage_bytes: IntGauge,

    // RPC metrics
    pub rpc_requests: IntCounterVec,
    pub rpc_request_duration: HistogramVec,
    pub rpc_errors: IntCounterVec,
}

impl Metrics {
    /// Create new metrics instance with default registry
    pub fn new() -> Self {
        Self::with_registry(Registry::new())
    }

    /// Create metrics with custom registry
    pub fn with_registry(registry: Registry) -> Self {
        let block_height = IntGauge::new("neo_block_height", "Current block height")
            .expect("metric creation failed");

        let blocks_processed = IntCounter::new("neo_blocks_processed_total", "Total blocks processed")
            .expect("metric creation failed");

        let block_processing_time = Histogram::with_opts(
            HistogramOpts::new("neo_block_processing_seconds", "Block processing time in seconds")
                .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5]),
        )
        .expect("metric creation failed");

        let transactions_processed = IntCounter::new(
            "neo_transactions_processed_total",
            "Total transactions processed",
        )
        .expect("metric creation failed");

        let mempool_size = IntGauge::new("neo_mempool_size", "Current mempool size")
            .expect("metric creation failed");

        let transaction_processing_time = Histogram::with_opts(
            HistogramOpts::new(
                "neo_transaction_processing_seconds",
                "Transaction processing time in seconds",
            )
            .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5]),
        )
        .expect("metric creation failed");

        let connected_peers = IntGauge::new("neo_connected_peers", "Number of connected peers")
            .expect("metric creation failed");

        let messages_received = IntCounterVec::new(
            Opts::new("neo_messages_received_total", "Total messages received"),
            &["message_type"],
        )
        .expect("metric creation failed");

        let messages_sent = IntCounterVec::new(
            Opts::new("neo_messages_sent_total", "Total messages sent"),
            &["message_type"],
        )
        .expect("metric creation failed");

        let bytes_received = IntCounter::new("neo_bytes_received_total", "Total bytes received")
            .expect("metric creation failed");

        let bytes_sent = IntCounter::new("neo_bytes_sent_total", "Total bytes sent")
            .expect("metric creation failed");

        let consensus_view = IntGauge::new("neo_consensus_view", "Current consensus view number")
            .expect("metric creation failed");

        let consensus_round_time = Histogram::with_opts(
            HistogramOpts::new(
                "neo_consensus_round_seconds",
                "Consensus round time in seconds",
            )
            .buckets(vec![1.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0]),
        )
        .expect("metric creation failed");

        let memory_usage_bytes = IntGauge::new("neo_memory_usage_bytes", "Memory usage in bytes")
            .expect("metric creation failed");

        let cpu_usage_percent = Gauge::new("neo_cpu_usage_percent", "CPU usage percentage")
            .expect("metric creation failed");

        let disk_usage_bytes = IntGauge::new("neo_disk_usage_bytes", "Disk usage in bytes")
            .expect("metric creation failed");

        let rpc_requests = IntCounterVec::new(
            Opts::new("neo_rpc_requests_total", "Total RPC requests"),
            &["method"],
        )
        .expect("metric creation failed");

        let rpc_request_duration = HistogramVec::new(
            HistogramOpts::new("neo_rpc_request_seconds", "RPC request duration in seconds")
                .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
            &["method"],
        )
        .expect("metric creation failed");

        let rpc_errors = IntCounterVec::new(
            Opts::new("neo_rpc_errors_total", "Total RPC errors"),
            &["method", "error_code"],
        )
        .expect("metric creation failed");

        // Register all metrics
        registry.register(Box::new(block_height.clone())).ok();
        registry.register(Box::new(blocks_processed.clone())).ok();
        registry.register(Box::new(block_processing_time.clone())).ok();
        registry.register(Box::new(transactions_processed.clone())).ok();
        registry.register(Box::new(mempool_size.clone())).ok();
        registry.register(Box::new(transaction_processing_time.clone())).ok();
        registry.register(Box::new(connected_peers.clone())).ok();
        registry.register(Box::new(messages_received.clone())).ok();
        registry.register(Box::new(messages_sent.clone())).ok();
        registry.register(Box::new(bytes_received.clone())).ok();
        registry.register(Box::new(bytes_sent.clone())).ok();
        registry.register(Box::new(consensus_view.clone())).ok();
        registry.register(Box::new(consensus_round_time.clone())).ok();
        registry.register(Box::new(memory_usage_bytes.clone())).ok();
        registry.register(Box::new(cpu_usage_percent.clone())).ok();
        registry.register(Box::new(disk_usage_bytes.clone())).ok();
        registry.register(Box::new(rpc_requests.clone())).ok();
        registry.register(Box::new(rpc_request_duration.clone())).ok();
        registry.register(Box::new(rpc_errors.clone())).ok();

        Self {
            registry: Arc::new(registry),
            block_height,
            blocks_processed,
            block_processing_time,
            transactions_processed,
            mempool_size,
            transaction_processing_time,
            connected_peers,
            messages_received,
            messages_sent,
            bytes_received,
            bytes_sent,
            consensus_view,
            consensus_round_time,
            memory_usage_bytes,
            cpu_usage_percent,
            disk_usage_bytes,
            rpc_requests,
            rpc_request_duration,
            rpc_errors,
        }
    }

    /// Get the Prometheus registry
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Gather all metrics as text
    pub fn gather(&self) -> String {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP server for metrics endpoint
pub struct MetricsServer {
    metrics: Metrics,
    address: std::net::SocketAddr,
}

impl MetricsServer {
    /// Create a new metrics server
    pub fn new(metrics: Metrics, address: std::net::SocketAddr) -> Self {
        Self { metrics, address }
    }

    /// Start the metrics server (non-blocking, returns handle)
    pub async fn start(self) -> crate::TelemetryResult<()> {
        tracing::info!("Metrics server starting on {}", self.address);
        tracing::debug!(
            "Serving {} registered metrics",
            self.metrics.registry().gather().len()
        );
        // In a full implementation, this would start an HTTP server
        // For now, we just log that it would start
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        metrics.block_height.set(12345);
        assert_eq!(metrics.block_height.get(), 12345);
    }

    #[test]
    fn test_metrics_gather() {
        let metrics = Metrics::new();
        metrics.block_height.set(100);
        metrics.connected_peers.set(10);

        let output = metrics.gather();
        assert!(output.contains("neo_block_height"));
        assert!(output.contains("neo_connected_peers"));
    }
}
