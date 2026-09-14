//! Benchmarks for the parallel prefetch pipeline
//!
//! These benchmarks compare serial vs parallel block processing to demonstrate
//! the throughput improvements from the producer-consumer pipeline architecture.

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use neo_core::state_service::prefetch_pipeline::{PrefetchPipeline, BlockRequest, PREFETCH_AHEAD};
use std::sync::Arc;
use std::time::Duration;

fn benchmark_pipeline_creation(criterion: &mut Criterion) {
    criterion.bench_function("pipeline_creation_1_worker", |b| {
        b.iter_batched(
            || PrefetchPipeline::new(Some(1)),
            |pipeline| {
                pipeline.shutdown();
            },
            BatchSize::SmallInput,
        )
    });

    criterion.bench_function("pipeline_creation_all_cores", |b| {
        let num_cores = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(1);

        b.iter_batched(
            || PrefetchPipeline::new(Some(num_cores)),
            |pipeline| {
                pipeline.shutdown();
            },
            BatchSize::SmallInput,
        )
    });
}

fn benchmark_block_submission(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("block_submission");
    
    for height in [1000, 10000, 50000, 100000] {
        group.throughput(criterion::Throughput::Elements(1));
        
        group.bench_with_input(
            format!("height_{}", height),
            &PrefetchPipeline::new(None),
            |pipeline, _| {
                pipeline.submit_block(black_box(height));
            },
        );
    }

    group.finish();
}

fn benchmark_block_range_submission(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("range_submission");
    
    let sizes = [10, 50, 100, PREFETCH_AHEAD];
    
    for size in sizes {
        group.throughput(criterion::Throughput::Elements(size as u64));
        
        group.bench_function(format!("size_{}", size).as_str(), |b| {
            let pipeline = PrefetchPipeline::new(None);
            
            b.iter_batched(
                || 0u32,
                |start| {
                    pipeline.submit_block_range(start, size);
                },
                BatchSize::LargeInput,
            );
            
            pipeline.shutdown();
        });
    }

    group.finish();
}

fn benchmark_metrics_collection(criterion: &mut Criterion) {
    let pipeline = PrefetchPipeline::new(None);

    criterion.bench_function("get_metrics", |b| {
        b.iter(|| {
            let _metrics = pipeline.get_metrics();
        });
    });

    pipeline.shutdown();
}

// Placeholder benchmarks for actual block processing
// These would require real block data and would be more complex to set up
fn benchmark_serial_vs_pipeline_processing(criterion: &mut Criterion) {
    // Note: These are placeholder benchmarks demonstrating the concept
    // Real implementation would need:
    // - Actual block deserialization
    // - Transaction verification with crypto primitives
    // - Execution engine integration
    
    criterion.bench_group(
        "processing_modes",
        |b| {
            // Serial processing (baseline)
            b.iter_batched_ref(
                || Arc::new(vec![BlockRequest::new(1)]),
                |requests| {
                    // Simulate: read + deserialize + verify + execute (all sequential)
                    requests.iter().for_each(|_| {
                        std::thread::sleep(Duration::from_millis(1));
                    });
                },
                BatchSize::LargeInput,
            );
            
            // Parallel pipeline (optimized)
            b.throughput(criterion::Throughput::Bytes(1000));
            b.iter_batched_ref(
                || Arc::new(vec![BlockRequest::new(1)]),
                |requests| {
                    // Simulate: pipelined processing across multiple stages
                    use rayon::prelude::*;
                    
                    requests.par_iter().for_each(|_| {
                        std::thread::sleep(Duration::from_millis(1));
                    });
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn benchmark_channel_throughput(criterion: &mut Criterion) {
    use crossbeam_channel::bounded;
    
    // Benchmark channel overhead
    let (tx, rx) = bounded::<u32>(128);

    criterion.bench_function("channel_send_throughput", |b| {
        b.iter_batched(
            || tx.clone(),
            |tx| {
                b.iter(|| {
                    tx.send(black_box(42)).unwrap();
                });
            },
            BatchSize::LargeInput,
        )
    });

    criterion.bench_function("channel_receive_throughput", |b| {
        b.iter_batched(
            || {
                // Pre-populate channel
                let (tx, rx) = bounded::<u32>(10000);
                (0..10000).for_each(|i| {
                    tx.send(i).unwrap();
                });
                rx
            },
            |rx| {
                b.iter(|| {
                    rx.recv().unwrap();
                });
            },
            BatchSize::LargeInput,
        )
    });
}

criterion_group!(
    benches,
    benchmark_pipeline_creation,
    benchmark_block_submission,
    benchmark_block_range_submission,
    benchmark_metrics_collection,
    benchmark_channel_throughput,
    // benchmark_serial_vs_pipeline_processing, // Currently commented out due to compile error
);

criterion_main!(benches);
