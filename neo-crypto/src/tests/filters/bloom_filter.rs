use super::*;

fn expected_bits(bit_size: usize, hash_functions: usize, tweak: u32, element: &[u8]) -> Vec<u8> {
    let mut bits = BloomBits::from_vec(vec![0u8; bit_size.div_ceil(8)]);
    for hash_index in 0..hash_functions {
        let seed = (hash_index as u32)
            .wrapping_mul(SEED_MULTIPLIER)
            .wrapping_add(tweak);
        let bit = (murmur::murmur32(element, seed) as usize) % bit_size;
        bits.set(bit, true);
    }
    bits.into_vec()
}

#[test]
fn add_uses_neo_murmur_seed_schedule_and_lsb_bit_layout() {
    let bit_size = 32;
    let hash_functions = 3;
    let tweak = 0x1234_5678;
    let element = b"neo-rs";

    let mut filter = BloomFilter::new(bit_size, hash_functions, tweak).expect("filter");
    filter.add(element);

    assert_eq!(
        filter.bits(),
        expected_bits(bit_size, hash_functions, tweak, element)
    );
    assert!(filter.check(element));
}

#[test]
fn add_is_idempotent_for_same_element() {
    let mut filter = BloomFilter::new(32, 3, 0x1234_5678).expect("filter");
    filter.add(b"neo-rs");
    let first = filter.bits();

    filter.add(b"neo-rs");

    assert_eq!(filter.bits(), first);
}

#[test]
fn check_returns_false_when_a_required_bit_is_missing() {
    let bit_size = 32;
    let hash_functions = 3;
    let tweak = 0x1234_5678;
    let element = b"neo-rs";
    let mut bits = expected_bits(bit_size, hash_functions, tweak, element);
    let first_set_bit = bits
        .iter()
        .enumerate()
        .find_map(|(byte_index, byte)| {
            (0..8)
                .find(|bit_index| byte & (1 << bit_index) != 0)
                .map(|bit_index| (byte_index, bit_index))
        })
        .expect("expected at least one set bit");
    bits[first_set_bit.0] &= !(1 << first_set_bit.1);

    let filter = BloomFilter::with_bits(bit_size, hash_functions, tweak, &bits)
        .expect("filter with one missing bit");

    assert!(!filter.check(element));
}

#[test]
fn with_bits_preserves_wire_bytes_and_ignores_extra_input_bytes() {
    let filter = BloomFilter::with_bits(10, 2, 7, &[0b1000_0001, 0b1111_1111, 0xff])
        .expect("filter with bits");

    assert_eq!(filter.bits(), vec![0b1000_0001, 0b1111_1111]);
    assert_eq!(filter.bit_size(), 10);
    assert_eq!(filter.hash_functions(), 2);
    assert_eq!(filter.tweak(), 7);
}

#[test]
fn with_bits_zero_pads_short_input_for_non_byte_aligned_filters() {
    let filter = BloomFilter::with_bits(10, 2, 7, &[0b1000_0001]).expect("filter");

    assert_eq!(filter.bits(), vec![0b1000_0001, 0]);
}

#[test]
fn constructor_rejects_empty_dimensions() {
    assert!(BloomFilter::new(0, 1, 0).is_err());
    assert!(BloomFilter::new(8, 0, 0).is_err());
}
