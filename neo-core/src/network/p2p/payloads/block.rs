// Copyright (C) 2015-2025 The Neo Project.
//
// block.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! P2P payload block — re-exported from neo-protocol with neo-core extension
//! traits for verification.

pub use neo_protocol::payloads::block::Block;

use super::witness::Witness;
use crate::UInt160;
use crate::persistence::DataCache;

mod verification;
pub use verification::BlockExt;

// ─── Local trait impls (neo-core specific) ──────────────────────────────────

impl crate::VerifiableExt for Block {
    fn script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        self.header.script_hashes_for_verifying(snapshot)
    }
}

impl crate::WitnessData for Block {
    fn witnesses(&self) -> Vec<&Witness> {
        crate::WitnessData::witnesses(&self.header)
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        crate::WitnessData::witnesses_mut(&mut self.header)
    }
}

#[cfg(test)]
mod tests {
    use super::super::header::{Header, HeaderExt};
    use super::super::signer::Signer;
    use super::super::transaction::Transaction;
    use super::*;
    use crate::UInt256;
    use crate::WitnessScope;
    use crate::ledger::HeaderCache;
    use crate::persistence::StoreCache;
    use crate::persistence::providers::memory_store::MemoryStore;
    use crate::persistence::store::Store;
    use crate::protocol_settings::ProtocolSettings;
    use ::neo_ledger::validation::rules::MIN_TIMESTAMP_MS;
    use neo_vm::OpCode;
    use std::sync::Arc;

    fn sample_header() -> Header {
        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(UInt256::from_bytes(&[1; 32]).expect("prev hash"));
        header.set_merkle_root(UInt256::from_bytes(&[2; 32]).expect("merkle root"));
        header.set_timestamp(1_700_000_000_000);
        header.set_nonce(0x0102_0304_0506_0708);
        header.set_index(42);
        header.set_primary_index(1);
        header.set_next_consensus(UInt160::from_bytes(&[3; 20]).expect("next consensus"));
        header.witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
        header
    }

    fn sample_block() -> Block {
        Block {
            header: sample_header(),
            transactions: Vec::new(),
        }
    }

    fn transaction_with_oversized_script() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(100_000_000);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn block_try_hash_delegates_to_header() {
        let mut block = sample_block();
        let mut header = block.header.clone();

        assert_eq!(
            block.try_hash().expect("block hash"),
            header.try_hash().unwrap()
        );
    }

    #[test]
    fn iverifiable_block_hash_uses_try_hash() {
        let block = sample_block();
        let mut expected_source = block.clone();
        let expected = expected_source.try_hash().expect("try hash");

        assert_eq!(
            <Block as neo_primitives::Verifiable>::hash(&block).unwrap(),
            expected
        );
    }

    #[test]
    fn block_try_get_hash_data_matches_header_hash_data() {
        let block = sample_block();

        assert_eq!(
            block.try_get_hash_data().expect("block hash data"),
            block.header.try_get_hash_data().expect("header hash data")
        );
    }

    #[test]
    fn try_rebuild_merkle_root_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());

        assert!(block.try_rebuild_merkle_root().is_err());
    }

    #[test]
    fn verify_merkle_root_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());
        block.header.set_merkle_root(UInt256::default());

        assert!(!block.verify_merkle_root());
    }

    #[test]
    fn duplicate_transaction_check_rejects_unserializable_transaction_hash() {
        let mut block = sample_block();
        block.transactions.push(transaction_with_oversized_script());

        assert!(!block.verify_no_duplicate_transactions());
    }

    #[test]
    fn verify_with_cache_delegates_to_header_without_transaction_checks() {
        let deterministic_witness =
            Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte(), OpCode::RET.byte()]);
        let consensus = deterministic_witness.script_hash();

        let mut previous = Header::new();
        previous.set_version(0);
        previous.set_index(0);
        previous.set_timestamp(MIN_TIMESTAMP_MS + 1_000);
        previous.set_primary_index(0);
        previous.witness = deterministic_witness.clone();
        previous.set_next_consensus(consensus);
        let mut previous_for_hash = previous.clone();
        let previous_hash = previous_for_hash.hash();

        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(previous_hash);
        header.set_index(1);
        header.set_timestamp(MIN_TIMESTAMP_MS + 2_000);
        header.set_primary_index(0);
        header.witness = deterministic_witness;
        header.set_merkle_root(UInt256::from([0xabu8; 32]));

        let block = Block {
            header,
            transactions: Vec::new(),
        };
        let header_cache = HeaderCache::new();
        header_cache.add(previous);
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let store_cache = StoreCache::new_from_store(store, false);
        let mut settings = ProtocolSettings::default_settings();
        settings.validators_count = 7;

        assert!(
            block
                .header
                .verify_with_cache(&settings, &store_cache, &header_cache)
        );
        assert!(block.verify_with_cache(&settings, &store_cache, &header_cache));
    }
}
