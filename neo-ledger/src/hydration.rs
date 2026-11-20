//! Hydration of the runtime ledger caches from persisted storage.
//!
//! Startup does not trust the persistent store: a store can be truncated, half
//! written, or edited between runs. Hydration therefore *reconstructs* the
//! in-memory ledger from the persisted blocks and *proves* that what it read is
//! still a chain before any runtime cache is mutated:
//!
//! 1. read the persisted current-block pointer (height and hash);
//! 2. locate the genesis index — an empty store with an untouched pointer is a
//!    valid no-op, a pointer without genesis is not;
//! 3. walk heights `0..=height`, checking for each one that the indexed hash
//!    exists, the stored block deserializes, the header's own index matches the
//!    height it is stored under, the header hashes to the indexed hash, and the
//!    previous-hash link points at the block read before it;
//! 4. check the tip against the persisted current hash;
//! 5. only then insert every block into [`LedgerContext`] and populate
//!    [`HeaderCache`], and finally record the tip.
//!
//! Because the whole chain is validated before step 5, a corrupt store can never
//! leave a caller with a partially hydrated ledger.
//!
//! Two entry points share this policy:
//!
//! * [`hydrate_ledger`] — full chain hydration for an online node;
//! * [`hydrate_ledger_tip_for_checkpoint_replay`] — tip-only hydration for a
//!   checkpoint-gated offline replay. The caller must authenticate the
//!   checkpoint separately; the persisted tip index, indexed hash, current-hash
//!   pointer and parent linkage are still verified here.
//!
//! The ledger domain owns this logic because every rule it applies is a ledger
//! rule; the node host only chooses the store provider and path.

use crate::{HeaderCache, LedgerContext};
use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
use neo_primitives::UInt256;
use neo_storage::persistence::StoreCache;
use std::fmt;
use std::sync::Arc;

/// Result alias for ledger hydration.
pub type LedgerHydrationResult<T> = Result<T, LedgerHydrationError>;

/// Persisted pointer a hydration read was issued for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerField {
    /// Persisted height of the chain tip.
    CurrentIndex,
    /// Persisted hash of the chain tip.
    CurrentHash,
    /// Indexed hash of height 0.
    GenesisIndex,
}

impl fmt::Display for PointerField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PointerField::CurrentIndex => "current block index",
            PointerField::CurrentHash => "current block hash",
            PointerField::GenesisIndex => "genesis block index",
        })
    }
}

/// Why persisted state cannot be hydrated into the runtime ledger.
///
/// Every variant names the persisted fact that failed and the height it failed
/// at, so an operator can tell a corrupt store from an unsupported one. The
/// `Display` strings are also the operator-facing diagnostics: they carry the
/// failing height and both hashes where a link is broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerHydrationError {
    /// A persisted pointer read (`current_index`, `current_hash`, ...) failed.
    PointerRead {
        /// Which pointer was being read.
        field: PointerField,
        /// Store-owned failure message.
        message: String,
    },
    /// The current-block pointer exists but no genesis block is indexed.
    PointerWithoutGenesis,
    /// The indexed hash lookup for a height failed.
    HashRead {
        /// Height whose indexed hash was being read.
        index: u32,
        /// Store-owned failure message.
        message: String,
    },
    /// The indexed hash of the persisted tip could not be read.
    TipHashRead {
        /// Tip height whose indexed hash was being read.
        index: u32,
        /// Store-owned failure message.
        message: String,
    },
    /// No block is stored at the height the chain claims.
    MissingBlock {
        /// Height that is missing from storage.
        index: u32,
    },
    /// A stored block could not be deserialized.
    BlockRead {
        /// Height of the block that failed to deserialize.
        index: u32,
        /// Store/codec-owned failure message.
        message: String,
    },
    /// The stored header reports a different height than the one it is indexed under.
    IndexMismatch {
        /// Height the block was stored under.
        index: u32,
        /// Height the stored header reports.
        actual: u32,
    },
    /// The stored header does not hash to the indexed hash.
    HashMismatch {
        /// Height whose hash disagrees.
        index: u32,
        /// Hash stored in the height index.
        indexed: UInt256,
        /// Hash computed from the stored header.
        computed: UInt256,
    },
    /// The previous-hash link to the parent block is broken.
    BrokenLink {
        /// Height whose `prev_hash` disagrees with its parent.
        index: u32,
        /// Hash of the block read immediately before this one.
        expected: UInt256,
        /// `prev_hash` recorded in this block's header.
        actual: UInt256,
    },
    /// The tip does not match the persisted current-hash pointer.
    TipHashMismatch {
        /// Hash stored in the current-block pointer.
        pointer: UInt256,
        /// Hash of the tip the walk ended on.
        tip: UInt256,
    },
    /// Inserting a validated block into the runtime cache failed.
    CacheInsert {
        /// Height whose insertion failed.
        index: u32,
        /// Ledger-owned failure message.
        message: String,
    },
    /// The persisted tip height, indexed hash and current hash disagree.
    InconsistentTip,
    /// The tip's parent is missing from storage.
    MissingTipParent,
    /// The tip's previous-hash link to its parent is broken.
    BrokenTipLink,
}

impl fmt::Display for LedgerHydrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerHydrationError::PointerRead { field, message } => {
                write!(f, "Failed to read {field}: {message}")
            }
            LedgerHydrationError::PointerWithoutGenesis => {
                f.write_str("Current block pointer exists without a genesis block")
            }
            LedgerHydrationError::HashRead { index, message } => {
                write!(f, "Failed to read block hash at height {index}: {message}")
            }
            LedgerHydrationError::TipHashRead { index, message } => {
                write!(f, "Failed to read tip hash at height {index}: {message}")
            }
            LedgerHydrationError::MissingBlock { index } => {
                write!(f, "Block at height {index} not found in storage")
            }
            LedgerHydrationError::BlockRead { index, message } => {
                write!(
                    f,
                    "Failed to deserialize block at height {index}: {message}"
                )
            }
            LedgerHydrationError::IndexMismatch { index, actual } => write!(
                f,
                "Block index mismatch at height {index}: stored header reports {actual}"
            ),
            LedgerHydrationError::HashMismatch {
                index,
                indexed,
                computed,
            } => write!(
                f,
                "Block hash mismatch at height {index}: index contains {indexed}, header hashes to {computed}"
            ),
            LedgerHydrationError::BrokenLink {
                index,
                expected,
                actual,
            } => write!(
                f,
                "Chain integrity broken at height {index}: expected prev_hash {expected}, got {actual}"
            ),
            LedgerHydrationError::TipHashMismatch { pointer, tip } => write!(
                f,
                "Current block hash mismatch: pointer contains {pointer}, tip hashes to {tip}"
            ),
            LedgerHydrationError::CacheInsert { index, message } => {
                write!(f, "Failed to insert hydrated block {index}: {message}")
            }
            LedgerHydrationError::InconsistentTip => {
                f.write_str("Persisted tip height, indexed hash, or current hash is inconsistent")
            }
            LedgerHydrationError::MissingTipParent => {
                f.write_str("Tip parent is missing from storage")
            }
            LedgerHydrationError::BrokenTipLink => {
                f.write_str("Persisted tip parent linkage is inconsistent")
            }
        }
    }
}

impl std::error::Error for LedgerHydrationError {}

/// Reads the persisted current-block pointer.
fn read_pointer(
    ledger_contract: &LedgerContract,
    store_cache: &StoreCache,
) -> LedgerHydrationResult<(u32, UInt256)> {
    let current_height = ledger_contract
        .current_index(store_cache)
        .map_err(|error| LedgerHydrationError::PointerRead {
            field: PointerField::CurrentIndex,
            message: error.to_string(),
        })?;
    let current_hash = ledger_contract.current_hash(store_cache).map_err(|error| {
        LedgerHydrationError::PointerRead {
            field: PointerField::CurrentHash,
            message: error.to_string(),
        }
    })?;
    Ok((current_height, current_hash))
}

/// Returns whether height 0 is indexed, rejecting a pointer without a genesis.
fn has_genesis(
    ledger_contract: &LedgerContract,
    store_cache: &StoreCache,
    current_height: u32,
    current_hash: &UInt256,
) -> LedgerHydrationResult<bool> {
    let first_hash = ledger_contract
        .get_block_hash_by_index(store_cache, 0)
        .map_err(|error| LedgerHydrationError::PointerRead {
            field: PointerField::GenesisIndex,
            message: error.to_string(),
        })?;

    if first_hash.is_some() {
        return Ok(true);
    }
    if current_height != 0 || current_hash != &UInt256::zero() {
        return Err(LedgerHydrationError::PointerWithoutGenesis);
    }
    Ok(false)
}

/// Hydrates the ledger from persisted storage.
///
/// Implements complete storage hydration as per C# Neo `Blockchain.Persist`:
/// loads every persisted block, validates chain integrity, then rebuilds the
/// ledger index and the header cache.
///
/// Reference: C# Neo `Blockchain.cs` initialization logic.
///
/// # Arguments
/// * `store_cache` - The store cache containing persisted data
/// * `ledger` - The ledger to hydrate
/// * `header_cache` - The header cache to populate
///
/// # Errors
/// Returns [`LedgerHydrationError`] if the genesis block is missing while a
/// pointer exists, a block or its indexed hash is missing, a block fails to
/// deserialize, a header disagrees with its height or indexed hash, a
/// previous-hash link is broken, the tip disagrees with the pointer, or the
/// runtime cache refuses a validated block. No runtime cache is mutated when an
/// error is returned.
pub fn hydrate_ledger(
    store_cache: &StoreCache,
    ledger: &Arc<LedgerContext>,
    header_cache: &Arc<HeaderCache>,
) -> LedgerHydrationResult<()> {
    let ledger_contract = LedgerContract::new();
    let (current_height, current_hash) = read_pointer(&ledger_contract, store_cache)?;
    if !has_genesis(&ledger_contract, store_cache, current_height, &current_hash)? {
        return Ok(());
    }

    let mut previous_hash = UInt256::zero();
    let mut blocks = Vec::with_capacity(current_height as usize + 1);
    for index in 0..=current_height {
        let indexed_hash = ledger_contract
            .get_block_hash_by_index(store_cache, index)
            .map_err(|error| LedgerHydrationError::HashRead {
                index,
                message: error.to_string(),
            })?
            .ok_or(LedgerHydrationError::MissingBlock { index })?;
        let block = ledger_contract
            .get_block(store_cache, HashOrIndex::Index(index))
            .map_err(|error| LedgerHydrationError::BlockRead {
                index,
                message: error.to_string(),
            })?
            .ok_or(LedgerHydrationError::MissingBlock { index })?;

        if block.index() != index {
            return Err(LedgerHydrationError::IndexMismatch {
                index,
                actual: block.index(),
            });
        }
        let block_hash = block.hash();
        if block_hash != indexed_hash {
            return Err(LedgerHydrationError::HashMismatch {
                index,
                indexed: indexed_hash,
                computed: block_hash,
            });
        }
        if block.prev_hash() != &previous_hash {
            return Err(LedgerHydrationError::BrokenLink {
                index,
                expected: previous_hash,
                actual: *block.prev_hash(),
            });
        }

        previous_hash = block_hash;
        blocks.push(neo_protocol::execution::conversions::convert_ledger_block(
            block,
        ));
    }

    if current_hash != previous_hash {
        return Err(LedgerHydrationError::TipHashMismatch {
            pointer: current_hash,
            tip: previous_hash,
        });
    }

    // The whole chain is validated, so nothing below can reject a block for a
    // chain-integrity reason; a cache refusal here is a runtime fault.
    for block in blocks {
        let index = block.index();
        let header = block.header.clone();
        ledger
            .insert_block(block)
            .map_err(|error| LedgerHydrationError::CacheInsert {
                index,
                message: error.to_string(),
            })?;
        header_cache.add(header);
    }
    ledger.record_tip(current_height);

    tracing::info!(
        target: "neo::ledger",
        height = current_height,
        "Storage hydration completed successfully"
    );
    Ok(())
}

/// Hydrates only the persisted tip for a checkpoint-gated offline replay.
///
/// This deliberately does not replace full chain hydration for online nodes. The
/// caller must independently authenticate the checkpoint and its audit history
/// before executing successors. The tip index, indexed hash, current-hash pointer,
/// and previous-block linkage are still checked against persistent storage.
///
/// # Errors
/// Returns [`LedgerHydrationError`] on the same conditions as
/// [`hydrate_ledger`], restricted to the tip and its parent.
pub fn hydrate_ledger_tip_for_checkpoint_replay(
    store_cache: &StoreCache,
    ledger: &Arc<LedgerContext>,
    header_cache: &Arc<HeaderCache>,
) -> LedgerHydrationResult<()> {
    let ledger_contract = LedgerContract::new();
    let (current_height, current_hash) = read_pointer(&ledger_contract, store_cache)?;
    if !has_genesis(&ledger_contract, store_cache, current_height, &current_hash)? {
        return Ok(());
    }

    let indexed_hash = ledger_contract
        .get_block_hash_by_index(store_cache, current_height)
        .map_err(|error| LedgerHydrationError::TipHashRead {
            index: current_height,
            message: error.to_string(),
        })?
        .ok_or(LedgerHydrationError::MissingBlock {
            index: current_height,
        })?;
    let tip = ledger_contract
        .get_block(store_cache, HashOrIndex::Index(current_height))
        .map_err(|error| LedgerHydrationError::BlockRead {
            index: current_height,
            message: error.to_string(),
        })?
        .ok_or(LedgerHydrationError::MissingBlock {
            index: current_height,
        })?;
    if tip.index() != current_height || tip.hash() != indexed_hash || indexed_hash != current_hash {
        return Err(LedgerHydrationError::InconsistentTip);
    }
    if current_height > 0 {
        let parent_index = current_height - 1;
        let previous = ledger_contract
            .get_block(store_cache, HashOrIndex::Index(parent_index))
            .map_err(|error| LedgerHydrationError::BlockRead {
                index: parent_index,
                message: error.to_string(),
            })?
            .ok_or(LedgerHydrationError::MissingTipParent)?;
        if previous.index() + 1 != current_height || tip.prev_hash() != &previous.hash() {
            return Err(LedgerHydrationError::BrokenTipLink);
        }
    }

    let tip_hash = tip.hash();
    let execution_tip = neo_protocol::execution::conversions::convert_ledger_block(tip);
    let header = execution_tip.header.clone();
    ledger
        .insert_block(execution_tip)
        .map_err(|error| LedgerHydrationError::CacheInsert {
            index: current_height,
            message: error.to_string(),
        })?;
    header_cache.add(header);
    ledger.record_tip(current_height);

    tracing::info!(
        target: "neo::ledger",
        height = current_height,
        tip = %tip_hash,
        "Checkpoint tip hydration completed successfully"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt160;
    use neo_protocol::execution::{Block, BlockHeader};
    use neo_protocol::witness::Witness;
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;

    fn block(index: u32, previous_hash: UInt256) -> Block {
        Block::new(
            BlockHeader::new(
                0,
                previous_hash,
                UInt256::zero(),
                u64::from(index),
                u64::from(index),
                index,
                0,
                UInt160::zero(),
                vec![Witness::new()],
            ),
            Vec::new(),
        )
    }

    fn chain(length: u32) -> Vec<Block> {
        let mut previous_hash = UInt256::zero();
        (0..length)
            .map(|index| {
                let block = block(index, previous_hash);
                previous_hash = block.hash();
                block
            })
            .collect()
    }

    fn persist_blocks(store_cache: &mut StoreCache, blocks: &[Block]) {
        let ledger_contract = LedgerContract::new();
        for block in blocks {
            ledger_contract
                .store_block_state(store_cache.data_cache(), block, &[])
                .expect("store block through the canonical ledger writer");
        }
        let tip = blocks.last().expect("at least one block");
        ledger_contract
            .set_current_block_state(store_cache.data_cache(), &tip.hash(), tip.index())
            .expect("store current block pointer");
        store_cache.commit();
    }

    fn empty_cache() -> StoreCache {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        StoreCache::new(store)
    }

    #[test]
    fn hydrate_empty_storage_is_noop() {
        let store_cache = empty_cache();
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());

        hydrate_ledger(&store_cache, &ledger, &header_cache).expect("empty store is valid");
        assert_eq!(ledger.current_height(), 0);
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn hydrate_single_genesis_block() {
        let mut store_cache = empty_cache();
        let blocks = chain(1);
        persist_blocks(&mut store_cache, &blocks);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        hydrate_ledger(&store_cache, &ledger, &header_cache).expect("hydrate genesis");

        assert_eq!(ledger.current_height(), 0);
        assert_eq!(ledger.block_hash_at(0), Some(blocks[0].hash()));
        assert_eq!(header_cache.count(), 1);
    }

    #[test]
    fn hydrate_complete_chain_indexes_every_height() {
        let mut store_cache = empty_cache();
        let blocks = chain(3);
        persist_blocks(&mut store_cache, &blocks);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        hydrate_ledger(&store_cache, &ledger, &header_cache).expect("hydrate complete chain");

        assert_eq!(ledger.current_height(), 2);
        for block in &blocks {
            assert_eq!(ledger.block_hash_at(block.index()), Some(block.hash()));
        }
        assert_eq!(header_cache.count(), 3);
    }

    #[test]
    fn checkpoint_replay_hydrates_only_the_authenticated_tip() {
        let mut store_cache = empty_cache();
        let blocks = chain(3);
        persist_blocks(&mut store_cache, &blocks);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        hydrate_ledger_tip_for_checkpoint_replay(&store_cache, &ledger, &header_cache)
            .expect("hydrate checkpoint tip");

        assert_eq!(ledger.current_height(), 2);
        assert_eq!(ledger.block_hash_at(2), Some(blocks[2].hash()));
        assert_eq!(ledger.block_hash_at(0), None);
        assert_eq!(ledger.block_hash_at(1), None);
        assert_eq!(header_cache.count(), 1);
    }

    #[test]
    fn checkpoint_replay_rejects_broken_tip_parent() {
        let mut store_cache = empty_cache();
        let genesis = block(0, UInt256::zero());
        let broken = block(1, UInt256::from([1u8; 32]));
        persist_blocks(&mut store_cache, &[genesis, broken]);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger_tip_for_checkpoint_replay(&store_cache, &ledger, &header_cache)
            .expect_err("broken tip parent must fail closed");
        assert_eq!(error, LedgerHydrationError::BrokenTipLink);
        assert!(error.to_string().contains("parent linkage"));
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn broken_chain_does_not_partially_hydrate() {
        let mut store_cache = empty_cache();
        let genesis = block(0, UInt256::zero());
        let broken = block(1, UInt256::from([1u8; 32]));
        persist_blocks(&mut store_cache, &[genesis, broken]);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect_err("broken previous-hash link must fail closed");

        assert!(matches!(
            error,
            LedgerHydrationError::BrokenLink { index: 1, .. }
        ));
        assert!(error.to_string().contains("Chain integrity broken"));
        assert!(ledger.block_hash_at(0).is_none());
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn hydrate_rejects_missing_block() {
        let mut store_cache = empty_cache();
        let blocks = chain(2);
        let ledger_contract = LedgerContract::new();
        ledger_contract
            .store_block_state(store_cache.data_cache(), &blocks[0], &[])
            .expect("store genesis");
        ledger_contract
            .set_current_block_state(store_cache.data_cache(), &blocks[1].hash(), 1)
            .expect("store tip beyond available blocks");
        store_cache.commit();

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect_err("missing indexed block must fail closed");

        assert_eq!(error, LedgerHydrationError::MissingBlock { index: 1 });
        assert!(error.to_string().contains("height 1 not found"));
        assert!(ledger.block_hash_at(0).is_none());
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn hydrate_rejects_tip_hash_mismatch() {
        let mut store_cache = empty_cache();
        let blocks = chain(2);
        let ledger_contract = LedgerContract::new();
        for block in &blocks {
            ledger_contract
                .store_block_state(store_cache.data_cache(), block, &[])
                .expect("store block");
        }
        ledger_contract
            .set_current_block_state(store_cache.data_cache(), &blocks[0].hash(), 1)
            .expect("store inconsistent tip");
        store_cache.commit();

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect_err("tip hash mismatch must fail closed");

        assert!(matches!(
            error,
            LedgerHydrationError::TipHashMismatch { .. }
        ));
        assert!(error.to_string().contains("Current block hash mismatch"));
        assert!(ledger.block_hash_at(0).is_none());
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn pointer_without_genesis_is_rejected() {
        let mut store_cache = empty_cache();
        let ledger_contract = LedgerContract::new();
        ledger_contract
            .set_current_block_state(store_cache.data_cache(), &UInt256::from([2u8; 32]), 4)
            .expect("store pointer without genesis");
        store_cache.commit();

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect_err("pointer without genesis must fail closed");

        assert_eq!(error, LedgerHydrationError::PointerWithoutGenesis);
        assert_eq!(header_cache.count(), 0);
    }
}
