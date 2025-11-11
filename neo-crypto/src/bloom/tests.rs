use super::{BloomError, BloomFilter};

#[test]
fn constructor_rejects_invalid_values() {
    assert_eq!(
        BloomFilter::new(0, 3, 123).unwrap_err(),
        BloomError::InvalidBitLength
    );
    assert_eq!(
        BloomFilter::new(3, 0, 123).unwrap_err(),
        BloomError::InvalidHashFunctionCount
    );
}

#[test]
fn constructor_sets_properties() {
    let filter = BloomFilter::new(7, 10, 123456).unwrap();
    assert_eq!(filter.m(), 7);
    assert_eq!(filter.k(), 10);
    assert_eq!(filter.tweak(), 123456);
}

#[test]
fn with_bits_handles_short_and_long_inputs() {
    let shorter = [0u8; 5];
    let filter = BloomFilter::with_bits(7, 10, 123456, &shorter).unwrap();
    assert_eq!(filter.m(), 7);
    assert_eq!(filter.k(), 10);

    let longer = [1u8; 16];
    let filter = BloomFilter::with_bits(7, 10, 123456, &longer).unwrap();
    assert_eq!(
        filter.bits().len(),
        super::filter::BloomFilter::bytes_for_bits(7)
    );
    assert!(filter.bits().iter().all(|&byte| byte == 1));
}

#[test]
fn add_and_check_behaves_like_csharp() {
    let mut filter = BloomFilter::new(7, 10, 123456).unwrap();
    let element = [0u8, 1, 2, 3, 4];
    filter.add(&element);
    assert!(filter.check(&element));
    let another = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    assert!(!filter.check(&another));
}

#[test]
fn copy_bits_returns_raw_buffer() {
    let mut filter = BloomFilter::new(7, 10, 123456).unwrap();
    filter.add(&[1, 2, 3, 4]);
    let mut buffer = [0u8; 7];
    filter.copy_bits(&mut buffer);
    assert_eq!(&buffer[..filter.bits().len()], filter.bits());
}
