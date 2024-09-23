use criterion::{black_box, criterion_group, criterion_main, Criterion};
use crate::util::Uint256;

fn bench_uint256_marshal_json(c: &mut Criterion) {
    let v = Uint256::from([0x01, 0x02, 0x03]);

    c.bench_function("Uint256 MarshalJSON", |b| {
        b.iter(|| {
            let _ = black_box(v.marshal_json());
        });
    });
}

criterion_group!(benches, bench_uint256_marshal_json);
criterion_main!(benches);
