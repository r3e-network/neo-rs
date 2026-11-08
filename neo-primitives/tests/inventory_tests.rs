use neo_primitives::{InventoryType, Inventory, SerializablePayload, UInt256};
use std::sync::Arc;

// Mock inventory implementation for testing
struct MockInventory {
    inv_type: InventoryType,
    hash: UInt256,
}

impl SerializablePayload for MockInventory {
    fn hash_data(&self) -> Vec<u8> {
        self.hash.to_bytes().to_vec()
    }

    fn hash(&self) -> UInt256 {
        self.hash.clone()
    }

    fn witness_count(&self) -> usize {
        0
    }

    fn invocation_script(&self, _index: usize) -> &[u8] {
        &[]
    }

    fn verification_script(&self, _index: usize) -> &[u8] {
        &[]
    }
}

impl Inventory for MockInventory {
    fn inventory_type(&self) -> InventoryType {
        self.inv_type
    }

    fn inventory_hash(&self) -> UInt256 {
        self.hash.clone()
    }
}

#[test]
fn inventory_type_returns_correct_type() {
    let inv = MockInventory {
        inv_type: InventoryType::Block,
        hash: UInt256::zero(),
    };

    assert_eq!(inv.inventory_type(), InventoryType::Block);
}

#[test]
fn inventory_hash_returns_correct_hash() {
    let hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
    let inv = MockInventory {
        inv_type: InventoryType::Transaction,
        hash: hash.clone(),
    };

    assert_eq!(inv.inventory_hash(), hash);
}

#[test]
fn inventory_hash_defaults_to_serializable_payload_hash() {
    let hash = UInt256::from_bytes(&[2u8; 32]).unwrap();
    let inv = MockInventory {
        inv_type: InventoryType::Block,
        hash: hash.clone(),
    };

    // Default implementation should call self.hash()
    assert_eq!(inv.inventory_hash(), inv.hash());
}

#[test]
fn inventory_with_transaction_type() {
    let inv = MockInventory {
        inv_type: InventoryType::Transaction,
        hash: UInt256::zero(),
    };

    assert_eq!(inv.inventory_type(), InventoryType::Transaction);
}

#[test]
fn inventory_with_extensible_type() {
    let inv = MockInventory {
        inv_type: InventoryType::Extensible,
        hash: UInt256::zero(),
    };

    assert_eq!(inv.inventory_type(), InventoryType::Extensible);
}

#[test]
fn inventory_trait_object_usage() {
    let inv: Arc<dyn Inventory> = Arc::new(MockInventory {
        inv_type: InventoryType::Block,
        hash: UInt256::zero(),
    });

    assert_eq!(inv.inventory_type(), InventoryType::Block);
    assert_eq!(inv.inventory_hash(), UInt256::zero());
}

#[test]
fn inventory_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MockInventory>();
}
