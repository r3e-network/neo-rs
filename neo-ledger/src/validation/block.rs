use super::header::{verify_header, verify_header_with_cache};
use super::header_cache::HeaderCache;
use super::rules::{
    BlockValidationError, validate_block_size, validate_block_version, validate_transaction_count,
    validate_wire_transaction_count_raw,
};
use neo_crypto::MerkleTree;
use neo_primitives::UInt256;
use neo_protocol::execution::Block as ExecutionBlock;
use neo_protocol::payloads::Block;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_storage::persistence::{PersistenceDataCache, StoreCache};

/// Checks that a block body is the one its header commits to, using the rules
/// C# `Block.Deserialize` enforces on the wire: supported version, at most
/// `ushort.MaxValue` transactions, no duplicate transaction hashes, and a
/// transaction merkle root equal to the header's (zero for an empty block).
///
/// Nothing here needs ledger state or executes a witness, so it is safe to run
/// on every ingress path, including trusted import where header verification
/// is skipped. Header signatures and witness execution live in
/// [`verify_header`] / [`verify_header_with_cache`]. The single-witness rule is
/// only observable on the execution representation; see
/// [`verify_execution_block_integrity`].
///
/// There is intentionally no byte-size bound: C# has none at acceptance. The
/// effective bounds are the transaction count above and whatever carried the
/// block (32 MiB for a P2P payload).
pub fn verify_block_integrity(block: &Block) -> Result<(), BlockValidationError> {
    validate_block_version(block.header.version())?;
    validate_wire_transaction_count_raw(block.transactions.len())?;
    if !block.verify_no_duplicate_transactions() {
        return Err(BlockValidationError::DuplicateTransactions);
    }
    if !block.verify_merkle_root() {
        let computed = block
            .transactions
            .iter()
            .map(neo_protocol::payloads::Transaction::try_hash)
            .collect::<Result<Vec<_>, _>>()
            .ok()
            .and_then(|hashes| MerkleTree::compute_root(&hashes))
            .unwrap_or_else(UInt256::zero);
        return Err(BlockValidationError::InvalidMerkleRoot {
            expected: *block.header.merkle_root(),
            computed,
        });
    }
    Ok(())
}

/// [`verify_block_integrity`] for an execution block, plus the wire rule that a
/// header carries exactly one witness.
///
/// The witness count must be checked here, before projecting: the payload
/// projection keeps only the first witness and fabricates an empty one when the
/// list is empty, so it cannot detect a bad count. This is an encoding check
/// only; the witness is neither executed nor cryptographically verified.
///
/// On success returns the payload projection, which is faithful once the count
/// is known to be one.
pub fn verify_execution_block_integrity(
    block: &ExecutionBlock,
) -> Result<Block, BlockValidationError> {
    let count = block.header.witnesses.len();
    if count != 1 {
        return Err(BlockValidationError::InvalidHeaderWitnessCount { count });
    }
    let payload_block = block.to_payload_block();
    verify_block_integrity(&payload_block)?;
    Ok(payload_block)
}

/// Consensus proposal policy: at most `MAX_TRANSACTIONS_PER_BLOCK` (512)
/// transactions and at most `MAX_BLOCK_SIZE` (2 MiB) serialized.
///
/// These mirror C# `ProtocolSettings.MaxTransactionsPerBlock` and the dBFT
/// plugin's `MaxBlockSize`, which bound what a consensus node will *propose* or
/// agree to sign. They are deliberately not part of block acceptance: a block
/// that carries a valid consensus witness is accepted by every C# node whatever
/// its size, so rejecting it here would stall this node on the chain everyone
/// else follows. Apply this only when building or validating a proposal; use
/// [`verify_block_integrity`] for acceptance.
pub fn verify_block_proposal_policy(block: &Block) -> Result<(), BlockValidationError> {
    validate_transaction_count(block)?;
    validate_block_size(block)
}

/// Block acceptance: body integrity plus full header verification against
/// ledger state. Proposal-policy limits are not applied here; see
/// [`verify_block_proposal_policy`].
pub fn verify_block(block: &Block, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
    verify_block_integrity(block).is_ok() && verify_header(&block.header, settings, store_cache)
}

/// [`verify_block`] with the parent resolved through the header cache.
pub fn verify_block_with_cache(
    block: &Block,
    settings: &ProtocolSettings,
    store_cache: &StoreCache,
    header_cache: &HeaderCache,
) -> bool {
    verify_block_integrity(block).is_ok()
        && verify_header_with_cache(&block.header, settings, store_cache, header_cache)
}

pub fn calculate_block_network_fee(block: &Block, _snapshot: &PersistenceDataCache) -> i64 {
    block
        .transactions
        .iter()
        .map(neo_protocol::payloads::Transaction::network_fee)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::super::rules::{BlockLike, MIN_TIMESTAMP_MS};
    use super::*;
    use neo_primitives::constants::{
        BLOCK_MAX_TX_WIRE_LIMIT, MAX_BLOCK_SIZE, MAX_TRANSACTIONS_PER_BLOCK,
    };
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

    /// Merkle root of a transaction list; the zero hash for an empty list, as
    /// in C# where an empty block carries `UInt256.Zero`.
    fn merkle_root_of(transactions: &[Transaction]) -> UInt256 {
        let hashes: Vec<UInt256> = transactions.iter().map(Transaction::hash).collect();
        MerkleTree::compute_root(&hashes).unwrap_or_else(UInt256::zero)
    }

    #[test]
    fn merkle_root_of_empty_transaction_list_is_zero() {
        assert_eq!(merkle_root_of(&[]), UInt256::zero());
        assert_ne!(merkle_root_of(&[Transaction::new()]), UInt256::zero());

        // An empty block built through the helper is body-consistent.
        let (header_cache, block) = verifiable_block(Vec::new());
        assert_eq!(verify_block_integrity(&block), Ok(()));
        assert!(verify_with(&header_cache, &block));
    }

    #[test]
    fn verify_block_integrity_accepts_empty_block_with_zero_merkle_root() {
        let (_, header) = header_with_cached_parent(UInt256::default());
        let block = Block {
            header,
            transactions: Vec::new(),
        };

        assert_eq!(verify_block_integrity(&block), Ok(()));
    }

    #[test]
    fn verify_block_integrity_rejects_non_zero_merkle_root_for_empty_block() {
        let root = UInt256::from([0xab; 32]);
        let (_, header) = header_with_cached_parent(root);
        let block = Block {
            header,
            transactions: Vec::new(),
        };

        assert_eq!(
            verify_block_integrity(&block),
            Err(BlockValidationError::InvalidMerkleRoot {
                expected: root,
                computed: UInt256::zero(),
            })
        );
    }

    #[test]
    fn verify_block_integrity_rejects_transactions_not_matching_merkle_root() {
        let root = UInt256::from([0xab; 32]);
        let transactions = vec![Transaction::new()];
        let computed = merkle_root_of(&transactions);
        let (_, header) = header_with_cached_parent(root);
        let block = Block {
            header,
            transactions,
        };

        assert_eq!(
            verify_block_integrity(&block),
            Err(BlockValidationError::InvalidMerkleRoot {
                expected: root,
                computed,
            })
        );
    }

    #[test]
    fn verify_block_integrity_rejects_duplicate_transactions() {
        let tx = Transaction::new();
        let transactions = vec![tx.clone(), tx];
        // The root matches the duplicated list, so only the duplicate rule
        // can reject this block.
        let (_, header) = header_with_cached_parent(merkle_root_of(&transactions));
        let block = Block {
            header,
            transactions,
        };
        assert!(block.verify_merkle_root());

        assert_eq!(
            verify_block_integrity(&block),
            Err(BlockValidationError::DuplicateTransactions)
        );
    }

    #[test]
    fn verify_block_integrity_rejects_unsupported_version() {
        let (_, mut header) = header_with_cached_parent(UInt256::default());
        header.set_version(1);
        let block = Block {
            header,
            transactions: Vec::new(),
        };

        assert_eq!(
            verify_block_integrity(&block),
            Err(BlockValidationError::UnsupportedVersion { version: 1 })
        );
    }

    #[test]
    fn verify_execution_block_integrity_requires_exactly_one_header_witness() {
        let (_, header) = header_with_cached_parent(UInt256::default());
        let block = Block {
            header,
            transactions: Vec::new(),
        };
        let mut execution = ExecutionBlock::from(&block);
        assert_eq!(execution.header.witnesses.len(), 1);
        assert!(verify_execution_block_integrity(&execution).is_ok());

        // The payload projection fabricates an empty witness here, so the
        // count has to be rejected on the execution header itself.
        execution.header.witnesses.clear();
        assert!(verify_block_integrity(&execution.to_payload_block()).is_ok());
        assert_eq!(
            verify_execution_block_integrity(&execution).map(|_| ()),
            Err(BlockValidationError::InvalidHeaderWitnessCount { count: 0 })
        );

        execution.header.witnesses = vec![Witness::new(), Witness::new()];
        assert_eq!(
            verify_execution_block_integrity(&execution).map(|_| ()),
            Err(BlockValidationError::InvalidHeaderWitnessCount { count: 2 })
        );
    }

    #[test]
    fn verify_execution_block_integrity_checks_body_against_header() {
        let (_, header) = header_with_cached_parent(UInt256::default());
        let block = Block {
            header,
            transactions: Vec::new(),
        };
        let mut execution = ExecutionBlock::from(&block);
        execution.transactions = vec![Transaction::new()];

        assert!(matches!(
            verify_execution_block_integrity(&execution),
            Err(BlockValidationError::InvalidMerkleRoot { .. })
        ));
    }

    #[test]
    fn verify_block_integrity_accepts_transactions_matching_merkle_root() {
        let transactions = vec![Transaction::new()];
        let (_, header) = header_with_cached_parent(merkle_root_of(&transactions));
        let block = Block {
            header,
            transactions,
        };

        assert_eq!(verify_block_integrity(&block), Ok(()));
    }

    fn distinct_transactions(count: usize, script_len: usize) -> Vec<Transaction> {
        (0..count)
            .map(|nonce| {
                let mut tx = Transaction::new();
                tx.set_nonce(u32::try_from(nonce).expect("nonce fits u32"));
                tx.set_script(vec![OpCode::NOP.byte(); script_len]);
                tx
            })
            .collect()
    }

    /// Builds a body-consistent block whose header verifies against the
    /// returned cache, so it exercises the full acceptance path.
    fn verifiable_block(transactions: Vec<Transaction>) -> (HeaderCache, Block) {
        let (header_cache, header) = header_with_cached_parent(merkle_root_of(&transactions));
        (
            header_cache,
            Block {
                header,
                transactions,
            },
        )
    }

    #[test]
    fn block_over_policy_transaction_count_is_accepted_but_fails_proposal_policy() {
        let count = MAX_TRANSACTIONS_PER_BLOCK + 1;
        let (header_cache, block) = verifiable_block(distinct_transactions(count, 1));

        assert_eq!(verify_block_integrity(&block), Ok(()));
        assert!(verify_with(&header_cache, &block));
        assert_eq!(
            verify_block_proposal_policy(&block),
            Err(BlockValidationError::TooManyTransactions {
                count,
                max_count: MAX_TRANSACTIONS_PER_BLOCK,
            })
        );
    }

    #[test]
    fn block_over_policy_size_is_accepted_but_fails_proposal_policy() {
        // 36 x ~60 KB scripts: over 2 MiB, well under the 512-transaction
        // policy count, so only the size limit can reject it.
        let (header_cache, block) = verifiable_block(distinct_transactions(36, 60_000));
        let size = BlockLike::size(&block);
        assert!(size > MAX_BLOCK_SIZE, "block size {size} must exceed 2 MiB");

        assert_eq!(verify_block_integrity(&block), Ok(()));
        assert!(verify_with(&header_cache, &block));
        assert_eq!(
            verify_block_proposal_policy(&block),
            Err(BlockValidationError::BlockTooLarge {
                size,
                max_size: MAX_BLOCK_SIZE,
            })
        );
    }

    #[test]
    fn block_within_policy_passes_proposal_policy() {
        let (_, block) = verifiable_block(distinct_transactions(MAX_TRANSACTIONS_PER_BLOCK, 1));
        assert_eq!(verify_block_proposal_policy(&block), Ok(()));
    }

    #[test]
    fn block_over_wire_transaction_count_is_rejected() {
        // 65,536 clones of one transaction: cheap to build, and the count rule
        // runs before the duplicate and merkle rules, so it is the one that
        // must fire. The exact 65,535 boundary is covered on
        // `validate_wire_transaction_count_raw`.
        let count = BLOCK_MAX_TX_WIRE_LIMIT + 1;
        let (_, header) = header_with_cached_parent(UInt256::default());
        let block = Block {
            header,
            transactions: vec![Transaction::new(); count],
        };

        assert_eq!(
            verify_block_integrity(&block),
            Err(BlockValidationError::TooManyTransactions {
                count,
                max_count: BLOCK_MAX_TX_WIRE_LIMIT,
            })
        );
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
