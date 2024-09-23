extern crate num_bigint;
extern crate test;

use num_bigint::BigInt;
use test::Bencher;

fn to_preallocated_bytes(value: &BigInt, buf: &mut [u8]) -> &[u8] {
    // Assuming the function `ToPreallocatedBytes` converts the BigInt to bytes and stores in buf
    // This is a placeholder implementation
    let bytes = value.to_bytes_be().1;
    buf.copy_from_slice(&bytes);
    &buf[..bytes.len()]
}

#[bench]
fn benchmark_to_preallocated_bytes(b: &mut Bencher) {
    let v = BigInt::from(100500);
    let vn = BigInt::from(-100500);
    let mut buf = vec![0u8; 4];

    b.iter(|| {
        test::black_box(to_preallocated_bytes(&v, &mut buf));
        test::black_box(to_preallocated_bytes(&vn, &mut buf));
    });
}
