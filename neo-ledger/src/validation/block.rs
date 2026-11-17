use super::header::{verify_header, verify_header_with_cache};
use super::header_cache::HeaderCache;
use super::rules::{validate_block_size, validate_transaction_count};
use neo_primitives::Verifiable;
use neo_protocol::payloads::Block;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_storage::persistence::{DataCache, StoreCache};

/// State-independent structural checks on a block payload: supported version,
/// transaction merkle root, duplicate transactions, size and count bounds.
///
/// Header signatures and witness execution need ledger state, so they live in
/// [`verify_header`] / [`verify_header_with_cache`]; callers must run both.
pub fn verify_block_structure(block: &Block) -> bool {
    block.verify()
        && validate_block_size(block).is_ok()
        && validate_transaction_count(block).is_ok()
}

pub fn verify_block(block: &Block, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
    verify_block_structure(block) && verify_header(&block.header, settings, store_cache)
}

pub fn verify_block_with_cache(
    block: &Block,
    settings: &ProtocolSettings,
    store_cache: &StoreCache,
    header_cache: &HeaderCache,
) -> bool {
    verify_block_structure(block)
        && verify_header_with_cache(&block.header, settings, store_cache, header_cache)
}

pub fn calculate_block_network_fee(block: &Block, _snapshot: &DataCache) -> i64 {
    block
        .transactions
        .iter()
        .map(neo_protocol::payloads::Transaction::network_fee)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::super::rules::MIN_TIMESTAMP_MS;
    use super::*;
    use neo_crypto::MerkleTree;
    use neo_primitives::UInt256;
    use neo_protocol::payloads::{Header, Transaction};
    use neo_protocol::witness::Witness;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;
    use neo_vm::OpCode;
    use std::sync::Arc;

    /// Builds a header cache whose verified tip is the parent of the returned
    /// header. The parent's next-consensus script is a deterministic
    /// `PUSHT; RET` witness, so header 1 can be verified without real keys.
    fn header_with_cached_parent(merkle_root: UInt256) -> (HeaderCache, Header) {
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
        let previous_hash = Header::hash(&mut previous_for_hash);

        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(previous_hash);
        header.set_index(1);
        header.set_timestamp(MIN_TIMESTAMP_MS + 2_000);
        header.set_primary_index(0);
        header.witness = deterministic_witness;
        header.set_merkle_root(merkle_root);

        let header_cache = HeaderCache::new();
        header_cache.add(previous);
        (header_cache, header)
    }

    fn default_settings() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default_settings();
        settings.validators_count = 7;
        settings
    }

    fn verify_with(header_cache: &HeaderCache, block: &Block) -> bool {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let store_cache = StoreCache::new_from_store(store, false);
        verify_block_with_cache(block, &default_settings(), &store_cache, header_cache)
    }

    fn merkle_root_of(transactions: &[Transaction]) -> UInt256 {
        let hashes: Vec<UInt256> = transactions.iter().map(Transaction::hash).collect();
        MerkleTree::compute_root(&hashes).expect("non-empty transaction list")
    }

    #[test]
    fn verify_block_structure_accepts_empty_block_with_zero_merkle_root() {
        let (_, header) = header_with_cached_parent(UInt256::default());
        let block = Block {
            header,
            transactions: Vec::new(),
        };

        assert!(verify_block_structure(&block));
    }

    #[test]
    fn verify_block_structure_rejects_transactions_not_matching_merkle_root() {
        let (_, header) = header_with_cached_parent(UInt256::from([0xab; 32]));
        let block = Block {
            header,
            transactions: vec![Transaction::new()],
        };

        assert!(!verify_block_structure(&block));
    }

    #[test]
    fn verify_block_structure_rejects_duplicate_transactions() {
        let tx = Transaction::new();
        let transactions = vec![tx.clone(), tx];
        let (_, header) = header_with_cached_parent(merkle_root_of(&transactions));
        let block = Block {
            header,
            transactions,
        };

        assert!(!verify_block_structure(&block));
    }

    #[test]
    fn verify_block_structure_accepts_transactions_matching_merkle_root() {
        let transactions = vec![Transaction::new()];
        let (_, header) = header_with_cached_parent(merkle_root_of(&transactions));
        let block = Block {
            header,
            transactions,
        };

        assert!(verify_block_structure(&block));
    }

    #[test]
    fn verify_block_with_cache_accepts_empty_block_with_zero_merkle_root() {
        let (header_cache, header) = header_with_cached_parent(UInt256::default());
        let block = Block {
            header,
            transactions: Vec::new(),
        };

        assert!(verify_with(&header_cache, &block));
    }

    #[test]
    fn verify_block_with_cache_rejects_transactions_not_matching_merkle_root() {
        let (header_cache, header) = header_with_cached_parent(UInt256::from([0xab; 32]));
        // Header witness is genuine, but the transaction list is not the one the
        // merkle root commits to: a peer must not be able to swap it in.
        let block = Block {
            header,
            transactions: vec![Transaction::new()],
        };

        assert!(!verify_with(&header_cache, &block));
    }

    #[test]
    fn verify_block_with_cache_rejects_duplicate_transactions() {
        let tx = Transaction::new();
        let transactions = vec![tx.clone(), tx];
        let root = merkle_root_of(&transactions);
        let (header_cache, header) = header_with_cached_parent(root);
        let block = Block {
            header,
            transactions,
        };

        assert!(!verify_with(&header_cache, &block));
    }

    #[test]
    fn verify_block_with_cache_accepts_transactions_matching_merkle_root() {
        let transactions = vec![Transaction::new()];
        let root = merkle_root_of(&transactions);
        let (header_cache, header) = header_with_cached_parent(root);
        let block = Block {
            header,
            transactions,
        };

        assert!(verify_with(&header_cache, &block));
    }
}
