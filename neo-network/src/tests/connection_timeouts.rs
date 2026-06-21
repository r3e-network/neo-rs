use super::*;

#[test]
fn defaults_match_csharp_connection_constants() {
    let timeouts = ConnectionTimeouts::default();
    assert_eq!(timeouts.initial, Duration::from_secs(10));
    assert_eq!(timeouts.idle, Duration::from_secs(60));
}
