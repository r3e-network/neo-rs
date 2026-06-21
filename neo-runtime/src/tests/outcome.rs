use super::*;

#[test]
fn execution_outcome_success_and_failure() {
    let hash = UInt256::default();
    let ok = ExecutionOutcome::success(hash, 1, 5_000);
    assert!(ok.ok);
    assert_eq!(ok.gas_consumed, 5_000);

    let bad = ExecutionOutcome::failure(hash, 1);
    assert!(!bad.ok);
    assert_eq!(bad.gas_consumed, 0);
}

#[test]
fn validation_result_ok_and_invalid() {
    let ok = ValidationResult::ok();
    assert!(ok.valid);
    assert!(ok.reason.is_none());

    let bad = ValidationResult::invalid("bad merkle root");
    assert!(!bad.valid);
    assert_eq!(bad.reason.as_deref(), Some("bad merkle root"));
}

#[test]
fn network_event_variants_are_distinct() {
    let hash = UInt256::default();
    let a = NetworkEvent::BlockReceived { block_hash: hash };
    let b = NetworkEvent::TransactionReceived { tx_hash: hash };
    assert_ne!(a, b);
}

#[test]
fn peer_connected_carries_optional_address() {
    let addr: SocketAddr = "203.0.113.7:20333".parse().expect("socket address");
    let with_addr = NetworkEvent::PeerConnected {
        peer_id: "peer:1".to_string(),
        address: Some(addr),
    };
    let without_addr = NetworkEvent::PeerConnected {
        peer_id: "peer:1".to_string(),
        address: None,
    };
    assert_ne!(with_addr, without_addr);

    match with_addr {
        NetworkEvent::PeerConnected { peer_id, address } => {
            assert_eq!(peer_id, "peer:1");
            assert_eq!(address, Some(addr));
        }
        other => panic!("unexpected variant: {other:?}"),
    }
}
