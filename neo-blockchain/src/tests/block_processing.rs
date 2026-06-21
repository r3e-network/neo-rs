use super::*;

#[test]
fn constants_have_expected_values() {
    // Sanity check: the drain batch size is bounded to keep cache pressure
    // predictable.
    assert!(DRAIN_BATCH_SIZE > 0);
    assert!(MAX_UNVERIFIED_CACHE_SIZE >= DRAIN_BATCH_SIZE);
}
