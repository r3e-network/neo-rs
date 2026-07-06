use super::*;

#[test]
fn constants_have_expected_values() {
    // Sanity check: the drain batch size is bounded to keep cache pressure
    // predictable.
    assert!(DRAIN_BATCH_SIZE > 0);
    assert!(MAX_UNVERIFIED_CACHE_SIZE >= DRAIN_BATCH_SIZE);
}

#[test]
fn batch_persist_resources_use_system_native_provider() {
    let source = include_str!("../../pipeline/block_processing/persist.rs");
    let start = source
        .find("pub(crate) fn batch_persist_resources")
        .expect("batch resource builder exists");
    let end = source[start..]
        .find("pub(crate) fn persist_block_sequence_with_resources")
        .map(|offset| start + offset)
        .expect("persist sequence follows resource builder");
    let builder = &source[start..end];

    assert!(
        builder.contains("self.system.native_contract_provider()"),
        "batch native-persistence resources must use the provider exposed by SystemContext"
    );
    assert!(
        builder.contains("NativePersistResources::from_provider"),
        "batch native-persistence resources must be created from the explicit provider"
    );
}
