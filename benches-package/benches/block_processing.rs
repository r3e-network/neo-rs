use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_block_processing(c: &mut Criterion) {
    c.bench_function("block_placeholder", |b| b.iter(|| black_box(42)));
}

criterion_group!(benches, benchmark_block_processing);
criterion_main!(benches);
