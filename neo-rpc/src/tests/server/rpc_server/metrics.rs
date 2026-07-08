use super::*;

#[test]
fn request_counter_increments() {
    let before = RPC_REQ_TOTAL.get();

    RPC_REQ_TOTAL.inc();

    assert!(RPC_REQ_TOTAL.get() >= before + 1.0);
}

#[test]
fn error_counter_increments() {
    let before = RPC_ERR_TOTAL.get();

    RPC_ERR_TOTAL.inc();

    assert!(RPC_ERR_TOTAL.get() >= before + 1.0);
}

#[test]
fn invalid_counter_metadata_is_noop() {
    static INVALID_COUNTER: RpcCounter = RpcCounter::new("invalid counter name", "Invalid");

    INVALID_COUNTER.inc();

    assert_eq!(INVALID_COUNTER.get(), 0.0);
}
