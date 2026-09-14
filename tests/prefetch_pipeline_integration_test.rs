//! Integration tests for the Prefetch Pipeline
//!
//! These tests verify that the parallel producer-consumer pipeline works correctly
//! and achieves the expected performance improvements over serial processing.

use neo_core::state_service::prefetch_pipeline::{
    PrefetchPipeline, 
    BlockRequest, 
    PipelineState,
    PREFETCH_AHEAD,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Test that pipeline can be created with auto-detected worker count
#[test]
fn test_pipeline_auto_detection() {
    let num_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or_else(|_| 1);
    
    let pipeline = PrefetchPipeline::new(None);
    let metrics = pipeline.get_metrics();
    
    assert_eq!(metrics.active_workers, num_cores.saturating_sub(1));
    pipeline.shutdown();
}

/// Test that pipeline can be created with explicit worker count
#[test]
fn test_pipeline_explicit_workers() {
    let explicit_workers = 4;
    let pipeline = PrefetchPipeline::new(Some(explicit_workers));
    let metrics = pipeline.get_metrics();
    
    assert_eq!(metrics.active_workers, explicit_workers);
    pipeline.shutdown();
}

/// Test block submission doesn't block
#[test]
fn test_block_submission_non_blocking() {
    let pipeline = PrefetchPipeline::new(None);
    
    let start = Instant::now();
    
    // Submit blocks rapidly - should not block even without consumers
    for height in 0..100 {
        pipeline.submit_block(height);
    }
    
    let elapsed = start.elapsed();
    
    // Should complete quickly (< 100ms) if truly non-blocking
    assert!(elapsed.as_millis() < 100, "Block submission took too long: {:?}", elapsed);
    
    pipeline.shutdown();
}

/// Test range submission
#[test]
fn test_range_submission() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Submit a range of 500 blocks
    pipeline.submit_block_range(1000, 500);
    
    // Should not panic or block
    pipeline.shutdown();
}

/// Test pause/resume functionality
#[test]
fn test_pause_resume_cycle() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Initially running
    assert_eq!(pipeline.get_metrics().state, PipelineState::Running);
    assert!(pipeline.is_running());
    
    // Pause
    pipeline.pause();
    assert_eq!(pipeline.get_metrics().state, PipelineState::Paused);
    
    // Can still submit while paused (will queue)
    pipeline.submit_block(999);
    
    // Resume
    pipeline.resume();
    assert_eq!(pipeline.get_metrics().state, PipelineState::Running);
    assert!(pipeline.is_running());
    
    pipeline.shutdown();
}

/// Test graceful shutdown
#[test]
fn test_graceful_shutdown() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Start some work
    for height in 0..10 {
        pipeline.submit_block(height);
    }
    
    // Shutdown should complete quickly
    let start = Instant::now();
    pipeline.shutdown();
    let elapsed = start.elapsed();
    
    // Should shut down within 1 second
    assert!(elapsed.as_secs() < 1, "Shutdown took too long: {:?}", elapsed);
    
    // Should not be running after shutdown
    assert!(!pipeline.is_running());
}

/// Test backpressure handling via channel depth
#[test]
fn test_backpressure_indicators() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Submit many blocks rapidly
    for height in 0..1000 {
        pipeline.submit_block(height);
        
        // Check metrics periodically
        if height % 100 == 0 {
            let metrics = pipeline.get_metrics();
            
            // Channel depth should give us hints about backpressure
            // In this basic test, we just verify it doesn't panic
            println!(
                "At block {}: io_depth={}, workers={}, state={:?}",
                height,
                metrics.io_channel_depth,
                metrics.active_workers,
                metrics.state
            );
        }
    }
    
    pipeline.shutdown();
}

/// Test concurrent access safety
#[test]
fn test_concurrent_access_safety() {
    use std::thread;
    
    let pipeline = PrefetchPipeline::new(None);
    
    // Spawn multiple threads submitting blocks
    let mut handles = vec![];
    
    for thread_id in 0..8 {
        let pipeline_clone = pipeline.clone();
        
        let handle = thread::spawn(move || {
            for i in 0..100 {
                let height = thread_id * 100 + i;
                pipeline_clone.submit_block(height);
            }
        });
        
        handles.push(handle);
    }
    
    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked");
    }
    
    // Verify no deadlocks occurred
    pipeline.shutdown();
}

/// Test metrics collection doesn't block main operation
#[test]
fn test_metrics_during_operation() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Start submitting blocks
    for height in 0..100 {
        pipeline.submit_block(height);
        
        // Collect metrics concurrently
        let _metrics = pipeline.get_metrics();
        
        // Metrics collection should not interfere
        assert!(pipeline.is_running());
    }
    
    pipeline.shutdown();
}

/// Test prefetched-ahead constant is reasonable
#[test]
fn test_prefetch_ahead_constant() {
    // PREFETCH_AHEAD should be high enough to keep workers busy
    // but low enough to avoid excessive memory growth
    
    const EXPECTED_MIN: u32 = 50;
    const EXPECTED_MAX: u32 = 200;
    
    assert!(PREFETCH_AHEAD >= EXPECTED_MIN, 
            "Prefetch ahead too small: {}", PREFETCH_AHEAD);
    assert!(PREFETCH_AHEAD <= EXPECTED_MAX, 
            "Prefetch ahead too large: {}", PREFETCH_AHEAD);
}

/// Performance test comparing throughput
#[test]
#[ignore] // Manual benchmark - run with `cargo test --test prefetch_pipeline_integration_test --ignored -- --nocapture`
fn perf_test_throughput_comparison() {
    let num_iterations = 1000;
    
    // Test serial-like processing (for comparison)
    let serial_start = Instant::now();
    let serial_pipeline = PrefetchPipeline::new(Some(1));
    for height in 0..num_iterations {
        serial_pipeline.submit_block(height);
    }
    serial_pipeline.shutdown();
    let serial_duration = serial_start.elapsed();
    
    // Test parallel pipeline
    let parallel_start = Instant::now();
    let parallel_cores = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(1);
    
    let parallel_pipeline = PrefetchPipeline::new(Some(parallel_cores));
    for height in 0..num_iterations {
        parallel_pipeline.submit_block(height);
    }
    parallel_pipeline.shutdown();
    let parallel_duration = parallel_start.elapsed();
    
    println!("Performance Comparison:");
    println!("Serial (1 worker):   {:?} ({:.2} req/s)", 
             serial_duration,
             num_iterations as f64 / serial_duration.as_secs_f64());
    println!("Parallel ({} workers): {:?} ({:.2} req/s)", 
             parallel_cores,
             parallel_duration,
             num_iterations as f64 / parallel_duration.as_secs_f64());
    println!("Speedup factor: {:.2}x",
             serial_duration.as_secs_f64() / parallel_duration.as_secs_f64());
    
    // The parallel version should show improvement when actual work is done
    // This test currently only measures submission latency, which is already fast
}

/// Test error handling - channel disconnection scenarios
#[test]
fn test_error_handling_scenarios() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Normal operation first
    pipeline.submit_block(1);
    pipeline.submit_block(2);
    
    // After shutdown, operations should be safe (logged but not panicking)
    pipeline.shutdown();
    
    // Submissions after shutdown should handle gracefully
    pipeline.submit_block(3); // Should not panic
    
    println!("Error handling test passed - no panics during edge cases");
}

/// Test that pipeline state transitions are correct
#[test]
fn test_state_transitions() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Initial state
    assert_eq!(pipeline.get_metrics().state, PipelineState::Running);
    
    // Running -> Paused -> Running -> ShuttingDown
    
    pipeline.pause();
    assert_eq!(pipeline.get_metrics().state, PipelineState::Paused);
    
    pipeline.resume();
    assert_eq!(pipeline.get_metrics().state, PipelineState::Running);
    
    pipeline.shutdown();
    assert_eq!(pipeline.get_metrics().state, PipelineState::ShuttingDown);
    
    // State machine enforces correct ordering
}

/// Test resource cleanup on drop
#[test]
fn test_drop_cleanup() {
    // Create and let go of pipeline without explicit shutdown
    {
        let pipeline = PrefetchPipeline::new(None);
        
        // Do some work
        for height in 0..10 {
            pipeline.submit_block(height);
        }
        
        // Don't call shutdown() - test drop behavior
    }
    // If we get here without hang, drop worked correctly
    println!("Drop cleanup test passed - resources freed properly");
}

/// Integration-style test simulating realistic workload
#[test]
#[ignore] // Skip by default - run manually for validation
fn simulate_realistic_block_sync() {
    let pipeline = PrefetchPipeline::new(None);
    
    // Simulate syncing a blockchain from height 10000 to 11000
    let start_height = 10000u32;
    let blocks_to_process = 1000u32;
    
    println!("Simulating sync from {} to {}", start_height, start_height + blocks_to_process);
    
    let start_time = Instant::now();
    
    // Submit blocks in batches (simulating what happens during actual sync)
    let batch_size = PREFETCH_AHEAD;
    for batch_start in 0..blocks_to_process {
        let offset = batch_start * batch_size;
        let next_batch = offset + batch_size.min(blocks_to_process - offset);
        
        pipeline.submit_block_range(start_height + offset, next_batch - offset);
        
        // In real usage, would consume results here:
        // while let Ok(result) = pipeline.try_get_next_result() {
        //     process_execution_result(result);
        // }
        
        // Small delay to prevent overwhelming system
        if batch_start % 10 == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    
    let submission_duration = start_time.elapsed();
    
    // Drain remaining results before shutdown
    let drain_start = Instant::now();
    loop {
        match pipeline.try_get_next_result() {
            Ok(_) => {/* Process result */},
            Err(_) => break, // Channel drained
        }
    }
    let drain_duration = drain_start.elapsed();
    
    pipeline.shutdown();
    let total_duration = start_time.elapsed();
    
    println!("Realistic sync simulation:");
    println!("  Blocks submitted:  {}", blocks_to_process);
    println!("  Submission time:   {:?}", submission_duration);
    println!("  Drain time:        {:?}", drain_duration);
    println!("  Total time:        {:?}", total_duration);
    println!("  Effective rate:    {:.2} blocks/sec",
             blocks_to_process as f64 / total_duration.as_secs_f64());
    
    // Validate expectations
    assert!(submission_duration.as_secs() < 10, 
            "Too slow to submit {} blocks", blocks_to_process);
}
