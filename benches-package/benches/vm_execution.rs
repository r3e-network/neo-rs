use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_vm_execution(c: &mut Criterion) {
    c.bench_function("vm_placeholder", |b| b.iter(|| black_box(42)));
}

criterion_group!(benches, benchmark_vm_execution);
criterion_main!(benches);
