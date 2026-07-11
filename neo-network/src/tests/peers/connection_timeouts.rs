use super::*;

#[test]
fn defaults_match_connection_and_fetch_liveness_policy() {
    let timeouts = ConnectionTimeouts::default();
    assert_eq!(timeouts.initial, Duration::from_secs(10));
    assert_eq!(timeouts.idle, Duration::from_secs(60));
    assert_eq!(timeouts.block_fetch, Duration::from_secs(15));
}
