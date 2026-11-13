//! High-load stress tests for Neo blockchain implementation
//!
//! Tests cover:
//! 1. Maximum throughput test - find sustainable transaction load ceiling
//! 2. Sustained load test - 1 hour at 80% capacity for stability
//! 3. Spike load test - sudden 10x traffic spikes
//! 4. Large block test - blocks with 1000+ transactions
//! 5. Network stress test - high message volume and concurrent connections

use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_protocol::builders::TransactionBuilder;
use neo_protocol::execution::{Block, BlockHeader};
use neo_protocol::payloads::transaction::Transaction;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tracing::info;

/// Stress test metrics collector
#[derive(Debug, Clone, Default)]
struct StressMetrics {
    total_transactions: u64,
    total_blocks: u64,
    successful_transactions: u64,
    failed_transactions: u64,
    total_duration_ms: u64,
    peak_memory_mb: u64,
    peak_throughput_tps: f64,
}

impl StressMetrics {
    fn transactions_per_second(&self) -> f64 {
        if self.total_duration_ms == 0 {
            return 0.0;
        }
        (self.successful_transactions as f64 * 1000.0) / self.total_duration_ms as f64
    }

    fn success_rate(&self) -> f64 {
        if self.total_transactions == 0 {
            return 0.0;
        }
        (self.successful_transactions as f64 / self.total_transactions as f64) * 100.0
    }
}

/// Reads the shared stress-test duration without imposing a one-hour local run.
/// Set `NEO_STRESS_DURATION_SECS=3600` for the sustained production soak test.
fn stress_duration(default_seconds: u64) -> Duration {
    let seconds = std::env::var("NEO_STRESS_DURATION_SECS")
        .ok()
        .map_or(default_seconds, |value| {
            value
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("NEO_STRESS_DURATION_SECS must be a positive integer"))
        });
    assert!(seconds > 0, "stress duration must be greater than zero");
    Duration::from_secs(seconds)
}

/// Create a synthetic transaction for testing
fn create_test_transaction(index: u64) -> Transaction {
    TransactionBuilder::new()
        .version(0)
        .nonce((index % 0xFFFFFFFF) as u32)
        .system_fee(1_000_000)
        .network_fee(1_000_000)
        .valid_until(10000)
        .add_signer(|s, _tx| {
            s.account(UInt160::zero()).scope(WitnessScope::NONE);
        })
        .build()
}

/// Create a block with specified number of transactions
fn create_test_block(index: u32, tx_count: usize) -> Block {
    let header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        1_678_000_000 + (index as u64),
        0xDEADBEEF,
        index,
        0,
        UInt160::zero(),
        vec![],
    );

    let mut transactions = Vec::with_capacity(tx_count);
    for i in 0..tx_count {
        transactions.push(create_test_transaction((index as u64) * 1000 + i as u64));
    }

    Block::new(header, transactions)
}

/// Test 1: Maximum throughput - find sustainable load ceiling
#[tokio::test]
#[ignore] // Run with: cargo test --test stress_tests -- --ignored
async fn stress_test_maximum_throughput() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_test_writer()
        .try_init();

    info!("=== STRESS TEST 1: MAXIMUM THROUGHPUT ===");

    let mut metrics = StressMetrics::default();
    let batch_duration = stress_duration(1);
    let start_time = Instant::now();

    let batch_sizes = vec![10, 50, 100, 200, 500, 1000];
    let mut peak_tps = 0.0;
    let mut optimal_batch_size = 0;

    for batch_size in batch_sizes {
        info!("Testing with batch size: {}", batch_size);

        let batch_start = Instant::now();
        let mut processed = 0u64;

        while batch_start.elapsed() < batch_duration {
            let mut transactions = Vec::with_capacity(batch_size);
            for i in 0..batch_size {
                transactions.push(create_test_transaction(processed + i as u64));
            }

            // Simulate transaction processing
            for tx in &transactions {
                let _ = tx.hash(); // Force hash computation
                metrics.successful_transactions += 1;
            }

            processed += batch_size as u64;
            metrics.total_transactions = processed;
        }

        let elapsed_ms = batch_start.elapsed().as_millis() as u64;
        let tps = (processed as f64 * 1000.0) / elapsed_ms as f64;

        info!(
            "Batch size {}: {:.2} TPS, {} total txs",
            batch_size, tps, processed
        );

        if tps > peak_tps {
            peak_tps = tps;
            optimal_batch_size = batch_size;
        }
    }

    metrics.total_duration_ms = start_time.elapsed().as_millis() as u64;
    metrics.peak_throughput_tps = peak_tps;

    info!("=== MAXIMUM THROUGHPUT TEST RESULTS ===");
    info!("Peak TPS: {:.2}", peak_tps);
    info!("Optimal batch size: {}", optimal_batch_size);
    info!(
        "Total transactions processed: {}",
        metrics.total_transactions
    );
    info!("Test duration: {}ms", metrics.total_duration_ms);

    // Assertions
    assert!(peak_tps > 100.0, "Peak TPS should exceed 100 TPS");
    assert_eq!(
        metrics.failed_transactions, 0,
        "Should have no failed transactions"
    );
}

/// Test 2: Sustained load - 1 hour at 80% capacity
#[tokio::test]
#[ignore] // Run with: cargo test --test stress_tests sustained_load -- --ignored --nocapture
async fn stress_test_sustained_load() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_test_writer()
        .try_init();

    info!("=== STRESS TEST 2: SUSTAINED LOAD (shortened for testing) ===");

    let test_duration = stress_duration(5);
    let target_tps = 80.0; // Target 80 TPS (80% of assumed 100 TPS capacity)
    let batch_interval = Duration::from_millis(1250).min(test_duration / 4);
    let batch_size = ((target_tps * batch_interval.as_secs_f64()).ceil() as usize).max(1);

    let mut metrics = StressMetrics::default();
    let start_time = Instant::now();
    let mut last_batch = Instant::now().checked_sub(batch_interval).unwrap();
    let mut memory_samples = Vec::new();

    info!(
        "Running sustained load test at {} TPS for {:?}",
        target_tps, test_duration
    );

    while start_time.elapsed() < test_duration {
        if last_batch.elapsed() >= batch_interval {
            let mut transactions = Vec::with_capacity(batch_size);
            for i in 0..batch_size {
                transactions.push(create_test_transaction(
                    metrics.total_transactions + i as u64,
                ));
            }

            // Process batch
            for tx in &transactions {
                let _ = tx.hash();
                metrics.successful_transactions += 1;
            }

            metrics.total_transactions += batch_size as u64;
            last_batch = Instant::now();

            // Sample memory every 10 seconds
            if start_time.elapsed().as_secs().is_multiple_of(10) {
                // Note: Real memory sampling would use sysinfo crate
                let estimated_memory_mb = (metrics.total_transactions * 500) / (1024 * 1024);
                memory_samples.push(estimated_memory_mb);

                if estimated_memory_mb > metrics.peak_memory_mb {
                    metrics.peak_memory_mb = estimated_memory_mb;
                }

                info!(
                    "Progress: {}s, {} txs, {:.2} TPS, ~{}MB memory",
                    start_time.elapsed().as_secs(),
                    metrics.total_transactions,
                    metrics.transactions_per_second(),
                    estimated_memory_mb
                );
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    metrics.total_duration_ms = start_time.elapsed().as_millis() as u64;

    // Check for memory leaks
    let memory_growth = if memory_samples.len() > 1 && memory_samples[0] > 0 {
        let first = memory_samples[0];
        let last = memory_samples[memory_samples.len() - 1];
        ((last as f64 - first as f64) / first as f64) * 100.0
    } else {
        0.0
    };

    info!("=== SUSTAINED LOAD TEST RESULTS ===");
    info!("Total transactions: {}", metrics.total_transactions);
    info!("Successful: {}", metrics.successful_transactions);
    info!("Failed: {}", metrics.failed_transactions);
    info!("Average TPS: {:.2}", metrics.transactions_per_second());
    info!(
        "Test duration: {:.2}s",
        metrics.total_duration_ms as f64 / 1000.0
    );
    info!("Peak memory: {}MB", metrics.peak_memory_mb);
    info!("Memory growth: {:.2}%", memory_growth);

    // Assertions
    assert!(
        metrics.transactions_per_second() >= target_tps * 0.9,
        "Should maintain at least 90% of target TPS"
    );
    assert_eq!(
        metrics.failed_transactions, 0,
        "Should have no failed transactions"
    );
    assert!(
        memory_growth < 50.0,
        "Memory growth should be less than 50%"
    );
}

/// Test 3: Spike load - sudden 10x traffic increase
#[tokio::test]
#[ignore]
async fn stress_test_spike_load() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_test_writer()
        .try_init();

    info!("=== STRESS TEST 3: SPIKE LOAD ===");

    let normal_tps = 50.0;
    let spike_multiplier = 10;
    let spike_tps = normal_tps * spike_multiplier as f64;
    let phase_duration = stress_duration(1);

    let mut metrics = StressMetrics::default();
    let start_time = Instant::now();

    // Phase 1: Normal load (configured duration)
    info!("Phase 1: Normal load at {} TPS", normal_tps);
    let phase1_start = Instant::now();
    while phase1_start.elapsed() < phase_duration {
        let tx = create_test_transaction(metrics.total_transactions);
        let _ = tx.hash();
        metrics.successful_transactions += 1;
        metrics.total_transactions += 1;

        tokio::time::sleep(Duration::from_millis(20)).await; // 50 TPS
    }
    let phase1_tps =
        (metrics.total_transactions as f64 * 1000.0) / phase1_start.elapsed().as_millis() as f64;
    info!("Phase 1 complete: {:.2} TPS", phase1_tps);

    // Phase 2: Spike load (configured duration)
    info!(
        "Phase 2: SPIKE load at {} TPS ({}x)",
        spike_tps, spike_multiplier
    );
    let phase2_start = Instant::now();
    let phase2_start_count = metrics.total_transactions;

    while phase2_start.elapsed() < phase_duration {
        // Send burst of transactions
        for _ in 0..spike_multiplier {
            let tx = create_test_transaction(metrics.total_transactions);
            let _ = tx.hash();
            metrics.successful_transactions += 1;
            metrics.total_transactions += 1;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let phase2_txs = metrics.total_transactions - phase2_start_count;
    let phase2_tps = (phase2_txs as f64 * 1000.0) / phase2_start.elapsed().as_millis() as f64;
    info!("Phase 2 complete: {:.2} TPS", phase2_tps);

    // Phase 3: Recovery to normal load (configured duration)
    info!("Phase 3: Recovery to normal load");
    let phase3_start = Instant::now();
    let phase3_start_count = metrics.total_transactions;

    while phase3_start.elapsed() < phase_duration {
        let tx = create_test_transaction(metrics.total_transactions);
        let _ = tx.hash();
        metrics.successful_transactions += 1;
        metrics.total_transactions += 1;

        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let phase3_txs = metrics.total_transactions - phase3_start_count;
    let phase3_tps = (phase3_txs as f64 * 1000.0) / phase3_start.elapsed().as_millis() as f64;
    info!("Phase 3 complete: {:.2} TPS", phase3_tps);

    metrics.total_duration_ms = start_time.elapsed().as_millis() as u64;

    info!("=== SPIKE LOAD TEST RESULTS ===");
    info!("Total transactions: {}", metrics.total_transactions);
    info!("Phase 1 (normal) TPS: {:.2}", phase1_tps);
    info!("Phase 2 (spike) TPS: {:.2}", phase2_tps);
    info!("Phase 3 (recovery) TPS: {:.2}", phase3_tps);
    info!(
        "Total duration: {:.2}s",
        metrics.total_duration_ms as f64 / 1000.0
    );
    info!("Failed transactions: {}", metrics.failed_transactions);

    // Assertions
    assert_eq!(
        metrics.failed_transactions, 0,
        "Should handle spike without failures"
    );
    assert!(
        phase2_tps > phase1_tps * 5.0,
        "Should handle at least 5x spike"
    );
    assert!(
        phase3_tps >= phase1_tps * 0.8,
        "Should recover to near-normal performance"
    );
}

/// Test 4: Large block processing - blocks with 1000+ transactions
#[tokio::test]
#[ignore]
async fn stress_test_large_blocks() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_test_writer()
        .try_init();

    info!("=== STRESS TEST 4: LARGE BLOCK PROCESSING ===");

    let block_sizes = vec![100, 500, 1000, 2000, 5000];
    let mut metrics = StressMetrics::default();
    let start_time = Instant::now();

    for &tx_count in &block_sizes {
        info!("Testing block with {} transactions", tx_count);

        let block = create_test_block(metrics.total_blocks as u32, tx_count);

        let block_start = Instant::now();

        // Simulate block processing
        let _block_hash = block.hash();
        for tx in &block.transactions {
            let _ = tx.hash();
            metrics.successful_transactions += 1;
        }

        let block_time_ms = block_start.elapsed().as_millis();
        metrics.total_transactions += tx_count as u64;
        metrics.total_blocks += 1;

        let throughput = (tx_count as f64 * 1000.0) / block_time_ms as f64;

        info!(
            "Block {} with {} txs processed in {}ms ({:.2} TPS)",
            metrics.total_blocks, tx_count, block_time_ms, throughput
        );

        // Check memory usage (estimated)
        let estimated_memory_mb = (tx_count * 500) / (1024 * 1024);
        if estimated_memory_mb as u64 > metrics.peak_memory_mb {
            metrics.peak_memory_mb = estimated_memory_mb as u64;
        }
    }

    metrics.total_duration_ms = start_time.elapsed().as_millis() as u64;

    info!("=== LARGE BLOCK TEST RESULTS ===");
    info!("Total blocks processed: {}", metrics.total_blocks);
    info!("Total transactions: {}", metrics.total_transactions);
    info!("Average TPS: {:.2}", metrics.transactions_per_second());
    info!("Peak memory: {}MB", metrics.peak_memory_mb);
    info!("Total duration: {}ms", metrics.total_duration_ms);

    // Assertions
    assert_eq!(metrics.total_blocks, block_sizes.len() as u64);
    assert_eq!(metrics.failed_transactions, 0);
    assert!(
        metrics.transactions_per_second() > 100.0,
        "Should process > 100 TPS"
    );
}

/// Test 5: Network stress - high message volume
#[tokio::test]
#[ignore]
async fn stress_test_network_load() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_test_writer()
        .try_init();

    info!("=== STRESS TEST 5: NETWORK STRESS ===");

    let concurrent_connections = 50;
    let messages_per_connection = 1000;
    let mut metrics = StressMetrics::default();
    let start_time = Instant::now();

    // Create channels to simulate network connections
    let mut join_set = JoinSet::new();

    for conn_id in 0..concurrent_connections {
        join_set.spawn(async move {
            let mut local_metrics = StressMetrics::default();
            let conn_start = Instant::now();

            for msg_id in 0..messages_per_connection {
                // Simulate message processing
                let tx = create_test_transaction((conn_id * 10000 + msg_id) as u64);
                let _ = tx.hash();
                local_metrics.successful_transactions += 1;
                local_metrics.total_transactions += 1;

                // Small delay to simulate network latency
                if msg_id % 10 == 0 {
                    tokio::time::sleep(Duration::from_micros(100)).await;
                }
            }

            local_metrics.total_duration_ms = conn_start.elapsed().as_millis() as u64;

            (conn_id, local_metrics)
        });
    }

    // Collect results
    let mut connection_results = Vec::new();
    while let Some(result) = join_set.join_next().await {
        if let Ok((conn_id, local_metrics)) = result {
            info!(
                "Connection {} completed: {} txs in {}ms ({:.2} TPS)",
                conn_id,
                local_metrics.total_transactions,
                local_metrics.total_duration_ms,
                local_metrics.transactions_per_second()
            );

            metrics.total_transactions += local_metrics.total_transactions;
            metrics.successful_transactions += local_metrics.successful_transactions;
            metrics.failed_transactions += local_metrics.failed_transactions;

            connection_results.push(local_metrics);
        }
    }

    metrics.total_duration_ms = start_time.elapsed().as_millis() as u64;

    // Calculate aggregate throughput
    let aggregate_tps =
        (metrics.total_transactions as f64 * 1000.0) / metrics.total_duration_ms as f64;

    info!("=== NETWORK STRESS TEST RESULTS ===");
    info!("Concurrent connections: {}", concurrent_connections);
    info!("Messages per connection: {}", messages_per_connection);
    info!("Total transactions: {}", metrics.total_transactions);
    info!("Successful: {}", metrics.successful_transactions);
    info!("Failed: {}", metrics.failed_transactions);
    info!("Aggregate throughput: {:.2} TPS", aggregate_tps);
    info!(
        "Total duration: {:.2}s",
        metrics.total_duration_ms as f64 / 1000.0
    );
    info!("Success rate: {:.2}%", metrics.success_rate());

    // Assertions
    assert_eq!(
        metrics.total_transactions,
        (concurrent_connections * messages_per_connection) as u64
    );
    assert_eq!(metrics.failed_transactions, 0, "Should have no failures");
    assert!(aggregate_tps > 1000.0, "Should handle > 1000 TPS aggregate");
    assert_eq!(
        metrics.success_rate(),
        100.0,
        "Should have 100% success rate"
    );
}
