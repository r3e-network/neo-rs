//! Native ledger contract: manages blocks and transactions on-chain.

use crate::smart_contract::native::NativeMethod;
use crate::{UInt160, UInt256};

/// Prefix for block-hash-by-index storage
const PREFIX_BLOCK_HASH: u8 = 9;
/// Prefix for block storage (trimmed block payloads)
const PREFIX_BLOCK: u8 = 5;
/// Prefix for transaction state storage
const PREFIX_TRANSACTION: u8 = 11;
/// Prefix for current block pointer storage
const PREFIX_CURRENT_BLOCK: u8 = 12;

pub(crate) mod keys;
mod metadata;
mod native_impl;
mod state;
mod storage;
pub use state::{LedgerTransactionStates, PersistedTransactionState};

/// LedgerContract native contract
pub struct LedgerContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl LedgerContract {
    pub const ID: i32 = -4;

    /// Creates a new LedgerContract instance
    pub fn new() -> Self {
        // LedgerContract hash: 0xda65b600f7124ce6c79950c1772a36403104f2be
        let hash = UInt160::parse("0xda65b600f7124ce6c79950c1772a36403104f2be")
            .expect("Valid LedgerContract hash");

        Self {
            id: Self::ID,
            hash,
            methods: Self::native_methods(),
        }
    }
}

/// Hash or index parameter for block queries
pub enum HashOrIndex {
    Hash(UInt256),
    Index(u32),
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use self::keys::{block_hash_storage_key, block_storage_key, transaction_storage_key};
    use super::*;
    use crate::ledger::{Block, BlockHeader};
    use crate::network::p2p::payloads::signer::Signer;
    use crate::network::p2p::payloads::witness::Witness;
    use crate::network::p2p::payloads::Transaction;
    use crate::persistence::DataCache;
    use crate::UInt160;
    use crate::WitnessScope;
    use neo_vm_rs::OpCode;
    use neo_vm_rs::VmState as VMState;

    fn make_signed_transaction() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_valid_until_block(10);
        tx.add_signer(Signer::new(
            UInt160::default(),
            WitnessScope::CALLED_BY_ENTRY,
        ));
        tx.add_witness(Witness::new());
        tx
    }

    fn make_unserializable_transaction() -> Transaction {
        let mut tx = make_signed_transaction();
        tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
        tx
    }

    #[test]
    fn update_vm_state_overwrites_persisted_value() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);

        let mut tx = make_signed_transaction();
        tx.set_script(vec![0xAA]);
        let state = PersistedTransactionState::new(&tx, 42);
        ledger
            .persist_transaction_state(&snapshot, &state)
            .expect("persist state");

        let hash = tx.hash();
        ledger
            .update_transaction_vm_state(&snapshot, &hash, VMState::HALT)
            .expect("update state");

        let stored = ledger
            .get_transaction_state(&snapshot, &hash)
            .expect("read state")
            .expect("state present");
        assert_eq!(stored.vm_state(), VMState::HALT);
        assert_eq!(stored.block_index(), 42);
    }

    #[test]
    fn batch_vm_state_update_applies_all_entries() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);

        let mut tx1 = make_signed_transaction();
        tx1.set_nonce(100);
        tx1.set_script(vec![0x01]);
        let mut tx2 = make_signed_transaction();
        tx2.set_nonce(200);
        tx2.set_script(vec![0x02]);

        let state1 = PersistedTransactionState::new(&tx1, 1);
        let state2 = PersistedTransactionState::new(&tx2, 2);
        ledger
            .persist_transaction_state(&snapshot, &state1)
            .expect("state1");
        ledger
            .persist_transaction_state(&snapshot, &state2)
            .expect("state2");

        let updates = vec![(tx1.hash(), VMState::FAULT), (tx2.hash(), VMState::HALT)];
        ledger
            .update_transaction_vm_states(&snapshot, &updates)
            .expect("updates");

        let state1 = ledger
            .get_transaction_state(&snapshot, &updates[0].0)
            .unwrap()
            .unwrap();
        let state2 = ledger
            .get_transaction_state(&snapshot, &updates[1].0)
            .unwrap()
            .unwrap();

        assert_eq!(state1.vm_state(), VMState::FAULT);
        assert_eq!(state2.vm_state(), VMState::HALT);
    }

    #[test]
    fn ledger_transaction_states_mark_vm_state() {
        let mut tx = Transaction::new();
        tx.set_script(vec![0x10]);
        let hash = tx.hash();
        let mut states = LedgerTransactionStates::new(vec![PersistedTransactionState::new(&tx, 0)]);
        let updated = states.mark_vm_state(&hash, VMState::FAULT);
        assert!(updated);
        let updates = states.try_into_updates().expect("updates");
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].1, VMState::FAULT);
    }

    #[test]
    fn persist_transaction_state_rejects_unserializable_hash_without_zero_key() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);
        let tx = make_unserializable_transaction();
        let state = PersistedTransactionState::new(&tx, 42);

        assert!(ledger.persist_transaction_state(&snapshot, &state).is_err());

        let zero_key = transaction_storage_key(ledger.id, &UInt256::zero());
        assert!(snapshot.get(&zero_key).is_none());
        assert!(snapshot.tracked_items().is_empty());
    }

    #[test]
    fn store_block_state_rejects_unserializable_transaction_before_tracking_writes() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);
        let tx = make_unserializable_transaction();
        let block = Block::new(BlockHeader::default(), vec![tx.clone()]);
        let tx_states = vec![PersistedTransactionState::new(&tx, block.index())];

        assert!(ledger
            .store_block_state(&snapshot, &block, &tx_states)
            .is_err());

        let block_hash = block.hash();
        assert!(snapshot
            .get(&block_hash_storage_key(ledger.id, block.index()))
            .is_none());
        assert!(snapshot
            .get(&block_storage_key(ledger.id, &block_hash))
            .is_none());
        assert!(snapshot
            .get(&transaction_storage_key(ledger.id, &UInt256::zero()))
            .is_none());
        assert!(snapshot.tracked_items().is_empty());
    }

    #[test]
    fn ledger_transaction_states_try_into_updates_rejects_unserializable_hash() {
        let tx = make_unserializable_transaction();
        let states = LedgerTransactionStates::new(vec![PersistedTransactionState::new(&tx, 0)]);

        assert!(states.try_into_updates().is_err());
    }

    #[test]
    fn traceable_block_window_matches_protocol_boundaries() {
        assert!(LedgerContract::is_traceable_block(100, 100, 1));
        assert!(LedgerContract::is_traceable_block(100, 91, 10));
        assert!(!LedgerContract::is_traceable_block(100, 90, 10));
        assert!(!LedgerContract::is_traceable_block(100, 101, 10));
    }

    #[test]
    fn conflict_stub_requires_traceable_base_and_matching_signer() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);
        let current_hash = UInt256::from_bytes(&[0xCC; 32]).unwrap();
        ledger
            .set_current_block_state(&snapshot, &current_hash, 100)
            .expect("current block");

        let conflict_hash = UInt256::from_bytes(&[0xAB; 32]).unwrap();
        let signer = UInt160::from_bytes(&[0x11; 20]).unwrap();
        let other_signer = UInt160::from_bytes(&[0x22; 20]).unwrap();
        ledger
            .persist_conflict_stub(&snapshot, &conflict_hash, 95, &[signer])
            .expect("persist conflict stub");

        assert!(ledger
            .contains_conflict_hash(&snapshot, &conflict_hash, &[signer], 10)
            .expect("matching signer"));
        assert!(!ledger
            .contains_conflict_hash(&snapshot, &conflict_hash, &[other_signer], 10)
            .expect("different signer"));
        assert!(!ledger
            .contains_conflict_hash(&snapshot, &conflict_hash, &[], 10)
            .expect("empty signers"));

        let stale_conflict = UInt256::from_bytes(&[0xCD; 32]).unwrap();
        ledger
            .persist_conflict_stub(&snapshot, &stale_conflict, 90, &[signer])
            .expect("persist stale conflict stub");
        assert!(!ledger
            .contains_conflict_hash(&snapshot, &stale_conflict, &[signer], 10)
            .expect("stale conflict"));

        let base_only_conflict = UInt256::from_bytes(&[0xEF; 32]).unwrap();
        ledger
            .persist_conflict_stub(&snapshot, &base_only_conflict, 95, &[])
            .expect("persist base-only conflict stub");
        assert!(!ledger
            .contains_conflict_hash(&snapshot, &base_only_conflict, &[signer], 10)
            .expect("base-only conflict"));
    }
}

impl LedgerContract {
    fn is_traceable_block(current_index: u32, target_index: u32, max_traceable: u32) -> bool {
        if target_index > current_index {
            return false;
        }
        let window_end = target_index.saturating_add(max_traceable);
        window_end > current_index
    }
}
