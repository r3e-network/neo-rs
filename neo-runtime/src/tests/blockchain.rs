use super::*;

#[test]
fn blockchain_event_equality_is_field_wise() {
    let a = BlockchainEvent::Imported {
        hash: UInt256::default(),
        height: 7,
        timestamp: 1_000,
    };
    let b = BlockchainEvent::Imported {
        hash: UInt256::default(),
        height: 7,
        timestamp: 1_000,
    };
    assert_eq!(a, b);
    assert_ne!(
        a,
        BlockchainEvent::Imported {
            hash: UInt256::default(),
            height: 8,
            timestamp: 1_000,
        }
    );
    assert_ne!(a, BlockchainEvent::Shutdown);
}
