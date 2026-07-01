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

#[test]
fn default_channels_absorb_fast_sync_bursts() {
    const TARGET_BPS_WINDOW: usize = 1000;
    const MIN_PEER_WINDOWS: usize = 4;

    assert!(
        DEFAULT_COMMAND_CAPACITY >= TARGET_BPS_WINDOW * MIN_PEER_WINDOWS,
        "command queue must absorb several 1000-block sync windows"
    );
    assert!(
        DEFAULT_EVENT_CAPACITY >= TARGET_BPS_WINDOW * MIN_PEER_WINDOWS,
        "event queue must absorb several 1000-block sync windows"
    );
}
