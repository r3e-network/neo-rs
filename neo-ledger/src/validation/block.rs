use super::header::{verify_header, verify_header_with_cache};
use super::header_cache::HeaderCache;
use neo_protocol::payloads::Block;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_storage::persistence::{DataCache, StoreCache};

pub fn verify_block(block: &Block, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
    verify_header(&block.header, settings, store_cache)
}

pub fn verify_block_with_cache(
    block: &Block,
    settings: &ProtocolSettings,
    store_cache: &StoreCache,
    header_cache: &HeaderCache,
) -> bool {
    verify_header_with_cache(&block.header, settings, store_cache, header_cache)
}

pub fn calculate_block_network_fee(block: &Block, _snapshot: &DataCache) -> i64 {
    block
        .transactions
        .iter()
        .map(|transaction| transaction.network_fee())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::super::rules::MIN_TIMESTAMP_MS;
    use super::*;
    use neo_primitives::UInt256;
    use neo_protocol::payloads::Header;
    use neo_protocol::witness::Witness;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;
    use neo_vm::OpCode;
    use std::sync::Arc;

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

        assert!(verify_header_with_cache(
            &block.header,
            &settings,
            &store_cache,
            &header_cache
        ));
        assert!(verify_block_with_cache(
            &block,
            &settings,
            &store_cache,
            &header_cache
        ));
    }
}
