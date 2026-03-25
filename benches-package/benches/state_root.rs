use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_state_root(c: &mut Criterion) {
    c.bench_function("state_root_placeholder", |b| b.iter(|| black_box(42)));
}

criterion_group!(benches, benchmark_state_root);
criterion_main!(benches);
