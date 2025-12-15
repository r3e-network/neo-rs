use neo_core::cryptography::bloom_filter::BloomFilter;
use neo_core::cryptography::CryptoError;

fn sample_data(id: u8) -> Vec<u8> {
    vec![id, id.wrapping_mul(3), id.wrapping_add(5)]
}

#[test]
fn bloom_filter_rejects_invalid_parameters() {
    let err = BloomFilter::new(0, 1, 123).expect_err("zero bit size should fail");
    matches_invalid_argument(err);

    let err = BloomFilter::new(8, 0, 123).expect_err("zero hash functions should fail");
    matches_invalid_argument(err);
}

#[test]
fn bloom_filter_add_and_check_round_trip() {
    let mut filter = BloomFilter::new(64, 3, 0xCAFE_BABE).expect("filter create");
    let element = sample_data(7);
    filter.add(&element);

    assert!(filter.check(&element));
    assert!(!filter.check(&sample_data(42)));
}

#[test]
fn bloom_filter_with_bits_rehydrates_state() {
    let mut original = BloomFilter::new(64, 4, 0x1234_5678).expect("filter create");
    let first = sample_data(1);
    let second = sample_data(2);
    original.add(&first);
    original.add(&second);

    let snapshot = original.bits();
    let restored = BloomFilter::with_bits(64, 4, 0x1234_5678, &snapshot).expect("rehydrate");

    assert!(restored.check(&first));
    assert!(restored.check(&second));
    assert_eq!(restored.hash_functions(), original.hash_functions());
    assert_eq!(restored.bit_size(), original.bit_size());
    assert_eq!(restored.tweak(), original.tweak());
}

fn matches_invalid_argument(err: CryptoError) {
    match err {
        CryptoError::InvalidArgument { .. } => {}
        other => panic!("expected invalid argument error, got {other:?}"),
    }
}
