//! Node-specific metrics collection
//!
//! This module provides Prometheus metrics for neo-node including:
//! - Blockchain metrics (block height, header height)
//! - Network metrics (peer count, timeouts)
//! - Mempool metrics (size)
//! - State root metrics
//! - Storage metrics (disk usage)
//! - Optimization metrics (Cuckoo hash, batch signatures, SIMD BLAKE2b)

use prometheus::{Counter, Encoder, Gauge, Histogram, HistogramOpts, HistogramVec, TextEncoder};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Current block height
pub static BLOCK_HEIGHT: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_block_height", "Current block height"));

/// Current header height
pub static HEADER_HEIGHT: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_header_height", "Highest header seen"));

/// Header lag (difference between header and block height)
pub static HEADER_LAG: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_header_lag", "Header lag in blocks"));

/// Mempool transaction count
pub static MEMPOOL_SIZE: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_mempool_size", "Mempool size (transactions)"));

/// Connected peer count
pub static PEER_COUNT: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_peer_count", "Number of connected peers"));

/// Handshake timeouts
pub static TIMEOUT_HANDSHAKE: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_p2p_timeouts_handshake", "Handshake timeouts"));

/// Read timeouts
pub static TIMEOUT_READ: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_p2p_timeouts_read", "Read timeouts"));

/// Write timeouts
pub static TIMEOUT_WRITE: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_p2p_timeouts_write", "Write timeouts"));

/// Local state root index
pub static STATE_LOCAL_ROOT_INDEX: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_state_local_root_index",
        "Current local state root index (block height) if known, otherwise -1",
    )
});

/// Validated state root index
pub static STATE_VALIDATED_ROOT_INDEX: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_state_validated_root_index",
        "Current validated state root index if known, otherwise -1",
    )
});

/// State root validation lag
pub static STATE_VALIDATED_LAG: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_state_validated_lag",
        "Difference between local and validated state roots; -1 when unknown",
    )
});

/// Total accepted state roots
pub static STATE_ROOT_INGEST_ACCEPTED: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_state_roots_accepted_total",
        "Total accepted state roots since process start",
    )
});

/// Total rejected state roots
pub static STATE_ROOT_INGEST_REJECTED: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_state_roots_rejected_total",
        "Total rejected state roots since process start",
    )
});

/// Counter for accepted state roots
pub static STATE_ROOT_INGEST_ACCEPTED_COUNTER: LazyLock<Counter> = LazyLock::new(|| {
    register_counter(
        "neo_state_roots_accepted",
        "Counter of accepted state roots since process start",
    )
});

/// Counter for rejected state roots
pub static STATE_ROOT_INGEST_REJECTED_COUNTER: LazyLock<Counter> = LazyLock::new(|| {
    register_counter(
        "neo_state_roots_rejected",
        "Counter of rejected state roots since process start",
    )
});

/// Free disk space
pub static DISK_FREE_BYTES: LazyLock<Gauge> =
    LazyLock::new(|| register_gauge("neo_storage_free_bytes", "Free bytes on storage path disk"));

/// Total disk space
pub static DISK_TOTAL_BYTES: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_storage_total_bytes",
        "Total bytes on storage path disk",
    )
});

// ============================================================================
// OPTIMIZATION METRICS (Phase 1-3 Optimizations)
// ============================================================================

/// Current sync height
pub static SYNC_CURRENT_HEIGHT: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_sync_current_height",
        "Current block height of the syncing node",
    )
});

/// Tip height (network maximum)
pub static SYNC_TIP_HEIGHT: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_sync_tip_height",
        "Tip height from network peers",
    )
});

/// Total transactions verified (counter)
pub static TRANSACTIONS_VERIFIED_COUNTER: LazyLock<Counter> = LazyLock::new(|| {
    register_counter(
        "neo_transactions_verified_total",
        "Total transactions verified since process start",
    )
});

/// Total blocks processed
pub static BLOCKS_PROCESSED_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter(
        "neo_blocks_processed_total",
        "Total blocks processed since process start",
    )
});

// Cuckoo Hash Metrics
pub static CUCKOO_HASH_LOOKUPS_HIT: LazyLock<Counter> = LazyLock::new(|| {
    register_counter(
        "neo_cuckoo_hash_lookups_total",
        "Total successful Cuckoo hash lookups",
    )
});

pub static CUCKOO_HASH_LOOKUPS_MISS: LazyLock<Counter> = LazyLock::new(|| {
    register_counter(
        "neo_cuckoo_hash_lookups_miss_total",
        "Total failed Cuckoo hash lookups",
    )
});

// Histogram for syscall lookup latency
pub static SYSCALL_LOOKUP_LATENCY_BUCKETS: LazyLock<prometheus::Histogram> = LazyLock::new(|| {
    let histogram = Histogram::with_opts(
        HistogramOpts::new(
            "get_syscall_lookup_latency_bucket",
            "Histogram of syscall lookup latency in seconds",
        )
    ).unwrap_or_else(|_| Histogram::with_opts(HistogramOpts::new("get_syscall_lookup_latency", "Fallback")).expect("fallback"));
    
    // Register the histogram but ignore any register errors
    let _ = prometheus::register(Box::new(histogram.clone()));
    histogram
});

/// Batch signatures verification total
pub static BATCH_SIGNATURES_VERIFIED_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    register_counter(
        "neo_batch_signatures_verified_total",
        "Total signatures verified via batch verification",
    )
});

/// Average batch size
pub static AVERAGE_BATCH_SIZE: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_average_batch_size",
        "Average number of signatures per batch",
    )
});

/// SIMD BLAKE2b throughput in bytes per second
pub static SIMD_BLAKE2B_THROUGHPUT_BYTES_PER_SECOND: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_simd_blake2b_throughput_bytes_per_second",
        "SIMD BLAKE2b hashing throughput in bytes/second",
    )
});

/// Prefetch stage latencies (histogram by stage)
pub static PREFETCH_STAGE_LATENCIES_BUCKETS: LazyLock<prometheus::HistogramVec> = LazyLock::new(|| {
    let histogram_vec = HistogramVec::new(
        HistogramOpts::new(
            "prefetch_stage_latencies_bucket",
            "Histogram of prefetch stage latencies in seconds",
        ),
        &["stage"],
    ).unwrap_or_else(|_| HistogramVec::new(
        HistogramOpts::new("prefetch_stage_latencies", "Fallback"),
        &["stage"],
    ).expect("fallback"));
    
    // Register the histogram vector but ignore any register errors
    let _ = prometheus::register(Box::new(histogram_vec.clone()));
    histogram_vec
});

// System metrics
pub static CPU_USAGE_RATIO: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "cpu_usage_ratio",
        "CPU usage ratio (0.0 to 1.0)",
    )
});

pub static MEMORY_USAGE_BYTES: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "memory_usage_bytes",
        "Memory usage in bytes",
    )
});

pub static SYSTEM_MEMORY_TOTAL_BYTES: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "system_memory_total_bytes",
        "Total system memory in bytes",
    )
});

/// Fast sync mode enabled flag
pub static FAST_SYNC_ENABLED: LazyLock<Gauge> = LazyLock::new(|| {
    register_gauge(
        "neo_fast_sync_enabled",
        "Flag indicating if fast sync mode is enabled (1=true, 0=false)",
    )
});

/// Block processing duration histogram
pub static BLOCK_PROCESSING_DURATION_BUCKETS: LazyLock<prometheus::Histogram> = LazyLock::new(|| {
    let histogram = Histogram::with_opts(
        HistogramOpts::new(
            "block_processing_duration",
            "Histogram of block processing duration in seconds",
        ).buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0])
    ).unwrap_or_else(|_| Histogram::with_opts(HistogramOpts::new("block_processing_duration", "Fallback")).expect("fallback"));
    
    // Register the histogram but ignore any register errors
    let _ = prometheus::register(Box::new(histogram.clone()));
    histogram
});

// Internal tracking for deltas
static STATE_ROOT_ACCEPTED_LAST: AtomicU64 = AtomicU64::new(0);
static STATE_ROOT_REJECTED_LAST: AtomicU64 = AtomicU64::new(0);

/// Helper to create and register a gauge
fn register_gauge(name: &str, help: &str) -> Gauge {
    let gauge = Gauge::new(name, help)
        .unwrap_or_else(|_| Gauge::new("neo_invalid", "Invalid").expect("fallback"));
    let _ = prometheus::register(Box::new(gauge.clone()));
    gauge
}

/// Helper to create and register a counter
fn register_counter(name: &str, help: &str) -> Counter {
    let counter = Counter::new(name, help)
        .unwrap_or_else(|_| Counter::new("neo_invalid", "Invalid").expect("fallback"));
    let _ = prometheus::register(Box::new(counter.clone()));
    counter
}

/// Update all node metrics
///
/// This is the main entry point for updating metrics from the node runtime.
#[allow(clippy::too_many_arguments)]
pub fn update_node_metrics(
    block_height: u32,
    header_height: u32,
    mempool_size: u32,
    peer_count: usize,
    state_local_root: Option<u32>,
    state_validated_root: Option<u32>,
    state_root_accepted: u64,
    state_root_rejected: u64,
) {
    // Update blockchain metrics
    BLOCK_HEIGHT.set(block_height as f64);
    HEADER_HEIGHT.set(header_height as f64);
    HEADER_LAG.set(header_height.saturating_sub(block_height) as f64);

    // Update mempool
    MEMPOOL_SIZE.set(mempool_size as f64);

    // Update network
    PEER_COUNT.set(peer_count as f64);

    // Update state root metrics
    STATE_LOCAL_ROOT_INDEX.set(state_local_root.map(|v| v as f64).unwrap_or(-1.0));
    STATE_VALIDATED_ROOT_INDEX.set(state_validated_root.map(|v| v as f64).unwrap_or(-1.0));

    let lag = match (state_local_root, state_validated_root) {
        (Some(local), Some(validated)) => local.saturating_sub(validated) as f64,
        _ => -1.0,
    };
    STATE_VALIDATED_LAG.set(lag);

    STATE_ROOT_INGEST_ACCEPTED.set(state_root_accepted as f64);
    STATE_ROOT_INGEST_REJECTED.set(state_root_rejected as f64);

    // Update counters based on deltas
    let prev_accepted = STATE_ROOT_ACCEPTED_LAST.swap(state_root_accepted, Ordering::Relaxed);
    let prev_rejected = STATE_ROOT_REJECTED_LAST.swap(state_root_rejected, Ordering::Relaxed);

    if state_root_accepted > prev_accepted {
        STATE_ROOT_INGEST_ACCEPTED_COUNTER.inc_by((state_root_accepted - prev_accepted) as f64);
    }
    if state_root_rejected > prev_rejected {
        STATE_ROOT_INGEST_REJECTED_COUNTER.inc_by((state_root_rejected - prev_rejected) as f64);
    }
}

/// Update timeout metrics
pub fn update_timeout_metrics(handshake: u64, read: u64, write: u64) {
    TIMEOUT_HANDSHAKE.set(handshake as f64);
    TIMEOUT_READ.set(read as f64);
    TIMEOUT_WRITE.set(write as f64);
}

/// Update storage metrics
pub fn update_storage_metrics(free_bytes: u64, total_bytes: u64) {
    DISK_FREE_BYTES.set(free_bytes as f64);
    DISK_TOTAL_BYTES.set(total_bytes as f64);
}

/// Update optimization-specific metrics (Phase 1-3)
#[allow(dead_code)] // Will be called from node when integrated
pub fn update_optimization_metrics(
    sync_current_height: u32,
    sync_tip_height: u32,
    transactions_verified: u64,
    blocks_processed: u64,
    cuckoo_lookups_hit: u64,
    cuckoo_lookups_miss: u64,
    syscall_lookup_latency_ns: f64,
    batch_signatures_verified: u64,
    average_batch_size: f64,
    blake2b_throughput_bytes_per_sec: f64,
    prefetch_io_latency_ns: f64,
    prefetch_verify_latency_ns: f64,
    prefetch_execute_latency_ns: f64,
    cpu_usage_ratio: f64,
    memory_usage_bytes: u64,
    system_memory_total_bytes: u64,
    fast_sync_enabled: bool,
    block_processing_duration_s: f64,
) {
    // Sync metrics
    SYNC_CURRENT_HEIGHT.set(sync_current_height as f64);
    SYNC_TIP_HEIGHT.set(sync_tip_height as f64);

    // Counters
    TRANSACTIONS_VERIFIED_COUNTER.inc_by(transactions_verified as f64);
    BLOCKS_PROCESSED_TOTAL.inc_by(blocks_processed as f64);

    // Cuckoo Hash metrics (use labels via separate counters)
    for _ in 0..cuckoo_lookups_hit {
        CUCKOO_HASH_LOOKUPS_HIT.inc();
    }
    for _ in 0..cuckoo_lookups_miss {
        CUCKOO_HASH_LOOKUPS_MISS.inc();
    }

    // Syscall lookup latency histogram
    SYSCALL_LOOKUP_LATENCY_BUCKETS.observe(syscall_lookup_latency_ns / 1e9); // Convert ns to s

    // Batch verification
    BATCH_SIGNATURES_VERIFIED_TOTAL.inc_by(batch_signatures_verified as f64);
    AVERAGE_BATCH_SIZE.set(average_batch_size);

    // SIMD BLAKE2b throughput
    SIMD_BLAKE2B_THROUGHPUT_BYTES_PER_SECOND.set(blake2b_throughput_bytes_per_sec);

    // Prefetch stage latencies
    PREFETCH_STAGE_LATENCIES_BUCKETS.with_label_values(&["io"]).observe(prefetch_io_latency_ns / 1e9);
    PREFETCH_STAGE_LATENCIES_BUCKETS.with_label_values(&["verify"]).observe(prefetch_verify_latency_ns / 1e9);
    PREFETCH_STAGE_LATENCIES_BUCKETS.with_label_values(&["execute"]).observe(prefetch_execute_latency_ns / 1e9);

    // System metrics
    CPU_USAGE_RATIO.set(cpu_usage_ratio.clamp(0.0, 1.0));
    MEMORY_USAGE_BYTES.set(memory_usage_bytes as f64);
    SYSTEM_MEMORY_TOTAL_BYTES.set(system_memory_total_bytes as f64);

    // Fast sync flag
    FAST_SYNC_ENABLED.set(if fast_sync_enabled { 1.0 } else { 0.0 });

    // Block processing duration
    BLOCK_PROCESSING_DURATION_BUCKETS.observe(block_processing_duration_s);
}

/// Gather all metrics in Prometheus text format
pub fn gather_prometheus() -> Vec<u8> {
    // Ensure all metrics are initialized
    let _ = &*BLOCK_HEIGHT;
    let _ = &*HEADER_HEIGHT;
    let _ = &*HEADER_LAG;
    let _ = &*MEMPOOL_SIZE;
    let _ = &*PEER_COUNT;
    let _ = &*TIMEOUT_HANDSHAKE;
    let _ = &*TIMEOUT_READ;
    let _ = &*TIMEOUT_WRITE;
    let _ = &*DISK_FREE_BYTES;
    let _ = &*DISK_TOTAL_BYTES;
    let _ = &*STATE_LOCAL_ROOT_INDEX;
    let _ = &*STATE_VALIDATED_ROOT_INDEX;
    let _ = &*STATE_VALIDATED_LAG;
    let _ = &*STATE_ROOT_INGEST_ACCEPTED;
    let _ = &*STATE_ROOT_INGEST_REJECTED;
    let _ = &*STATE_ROOT_INGEST_ACCEPTED_COUNTER;
    let _ = &*STATE_ROOT_INGEST_REJECTED_COUNTER;
    
    // Optimization metrics
    let _ = &*SYNC_CURRENT_HEIGHT;
    let _ = &*SYNC_TIP_HEIGHT;
    let _ = &*TRANSACTIONS_VERIFIED_COUNTER;
    let _ = &*BLOCKS_PROCESSED_TOTAL;
    let _ = &*CUCKOO_HASH_LOOKUPS_HIT;
    let _ = &*CUCKOO_HASH_LOOKUPS_MISS;
    let _ = &*SYSCALL_LOOKUP_LATENCY_BUCKETS;
    let _ = &*BATCH_SIGNATURES_VERIFIED_TOTAL;
    let _ = &*AVERAGE_BATCH_SIZE;
    let _ = &*SIMD_BLAKE2B_THROUGHPUT_BYTES_PER_SECOND;
    let _ = &*PREFETCH_STAGE_LATENCIES_BUCKETS;
    let _ = &*CPU_USAGE_RATIO;
    let _ = &*MEMORY_USAGE_BYTES;
    let _ = &*SYSTEM_MEMORY_TOTAL_BYTES;
    let _ = &*FAST_SYNC_ENABLED;
    let _ = &*BLOCK_PROCESSING_DURATION_BUCKETS;

    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap_or(());
    buffer
}

/// Container for all node metrics
#[derive(Debug, Default)]
pub struct NodeMetrics {
    pub blockchain: BlockMetrics,
    pub mempool: MempoolMetrics,
    pub network: NetworkMetrics,
    pub state_root: StateRootMetrics,
    pub storage: StorageMetrics,
}

/// Blockchain-specific metrics
#[derive(Debug, Default)]
pub struct BlockMetrics {
    pub block_height: u32,
    pub header_height: u32,
}

/// Mempool metrics
#[derive(Debug, Default)]
pub struct MempoolMetrics {
    pub size: u32,
}

/// Network metrics
#[derive(Debug, Default)]
pub struct NetworkMetrics {
    pub peer_count: usize,
    pub handshake_timeouts: u64,
    pub read_timeouts: u64,
    pub write_timeouts: u64,
}

/// State root metrics
#[derive(Debug, Default)]
pub struct StateRootMetrics {
    pub local_root_index: Option<u32>,
    pub validated_root_index: Option<u32>,
    pub accepted_total: u64,
    pub rejected_total: u64,
}

/// Storage metrics
#[derive(Debug, Default)]
pub struct StorageMetrics {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

impl NodeMetrics {
    /// Update all Prometheus metrics from this snapshot
    pub fn update_prometheus(&self) {
        update_node_metrics(
            self.blockchain.block_height,
            self.blockchain.header_height,
            self.mempool.size,
            self.network.peer_count,
            self.state_root.local_root_index,
            self.state_root.validated_root_index,
            self.state_root.accepted_total,
            self.state_root.rejected_total,
        );
        update_timeout_metrics(
            self.network.handshake_timeouts,
            self.network.read_timeouts,
            self.network.write_timeouts,
        );
        update_storage_metrics(self.storage.free_bytes, self.storage.total_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_node_metrics() {
        update_node_metrics(
            100,       // block_height
            105,       // header_height
            50,        // mempool_size
            10,        // peer_count
            Some(100), // state_local_root
            Some(95),  // state_validated_root
            1000,      // state_root_accepted
            10,        // state_root_rejected
        );

        assert_eq!(BLOCK_HEIGHT.get(), 100.0);
        assert_eq!(HEADER_HEIGHT.get(), 105.0);
        assert_eq!(HEADER_LAG.get(), 5.0);
    }

    #[test]
    fn test_gather_prometheus() {
        // Set a value to ensure metrics are registered
        BLOCK_HEIGHT.set(100.0);
        let output = gather_prometheus();
        let text = String::from_utf8(output).expect("valid UTF-8");
        assert!(text.contains("neo_block_height"));
    }
}
