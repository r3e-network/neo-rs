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
fn relay_result_event_is_field_wise() {
    let event = BlockchainEvent::RelayResult {
        hash: UInt256::from([7u8; 32]),
        inventory_type: neo_payloads::InventoryType::Transaction,
        block_index: Some(11),
        result: neo_primitives::VerifyResult::Invalid,
    };

    assert_eq!(
        event,
        BlockchainEvent::RelayResult {
            hash: UInt256::from([7u8; 32]),
            inventory_type: neo_payloads::InventoryType::Transaction,
            block_index: Some(11),
            result: neo_primitives::VerifyResult::Invalid,
        }
    );
    assert_ne!(
        event,
        BlockchainEvent::RelayResult {
            hash: UInt256::from([7u8; 32]),
            inventory_type: neo_payloads::InventoryType::Extensible,
            block_index: Some(11),
            result: neo_primitives::VerifyResult::Invalid,
        }
    );
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
