//! Block verification + persistence loop.
//!
//! The service accepts blocks from peers/consensus, parks out-of-order blocks
//! in a bounded unverified cache, and drains consecutive parked blocks as soon
//! as their parent lands.
//!
//! The native-contract half of C# `Blockchain.Persist` IS wired:
//! when the [`crate::service_context::SystemContext`] exposes a
//! store snapshot, [`BlockchainService::persist_block_sequence`]
//! runs [`crate::native_persist::persist_block_natives`] (genesis
//! initialization + `OnPersist` + `PostPersist` native hooks) over
//! it.

use std::sync::Arc;

use neo_payloads::Block;
use neo_state_service::mpt_store::MptChange;
use neo_storage::TrackState;
use tracing::{debug, error, warn};

use crate::internal::UnverifiedBlock;
use crate::service::BlockchainService;
use crate::service_context::SystemContext;

const DRAIN_BATCH_SIZE: usize = 50;
const MAX_UNVERIFIED_CACHE_SIZE: usize = 20_000;
const LEDGER_CONTRACT_ID: i32 = -4;

fn state_root_changes(snapshot: &neo_storage::DataCache) -> Vec<MptChange> {
    snapshot
        .tracked_items()
        .into_iter()
        .filter(|(key, _)| key.id() != LEDGER_CONTRACT_ID)
        .filter_map(|(key, trackable)| match trackable.state {
            TrackState::Added | TrackState::Changed => Some(MptChange::Put {
                key: key.to_array(),
                value: trackable.item.value_bytes().into_owned(),
            }),
            TrackState::Deleted => Some(MptChange::Delete {
                key: key.to_array(),
            }),
            TrackState::None | TrackState::NotFound => None,
        })
        .collect()
}

pub(crate) fn apply_state_root_changes(
    system: &dyn SystemContext,
    block_index: u32,
    snapshot: &neo_storage::DataCache,
) -> bool {
    let Some(mpt) = system.state_store().and_then(|store| store.mpt()) else {
        return true;
    };

    let changes = state_root_changes(snapshot);
    let root_before = mpt.current_local_root_hash();
    if let Err(err) = mpt.apply_block_changes(block_index, root_before, &changes) {
        error!(target: "neo", %err, "state-root MPT update failed");
        return false;
    }

    true
}

impl BlockchainService {
    pub(crate) fn park_unverified_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> bool {
        if self.unverified_block_count() >= MAX_UNVERIFIED_CACHE_SIZE {
            warn!(
                target: "neo",
                index = block.index(),
                "unverified block cache is full; dropping future block"
            );
            return false;
        }

        let index = block.index();
        let mut cache = self.unverified_blocks.lock();
        cache
            .entry(index)
            .or_default()
            .push_back(UnverifiedBlock::new(block, relay, pre_verified));
        true
    }

    fn pop_next_unverified_block(&self) -> Option<UnverifiedBlock> {
        let next_index = self.ledger.current_height().saturating_add(1);
        let mut cache = self.unverified_blocks.lock();
        let (next, empty) = {
            let list = cache.get_mut(&next_index)?;
            let next = list.pop_front();
            (next, list.is_empty())
        };
        if empty {
            cache.remove(&next_index);
        }
        next
    }

    /// Persist a consecutive block sequence: run the C#
    /// `Blockchain.Persist` pipeline (native OnPersist + ledger
    /// records, per-transaction Application execution, native
    /// PostPersist) when the system context exposes a store snapshot.
    /// The pipeline stages all writes in a child cache and commits
    /// them into the snapshot only when the whole sequence succeeds
    /// (see [`crate::native_persist`]). Store-less contexts are reserved for
    /// lightweight tests that exercise the command loop without durable
    /// native-contract persistence.
    pub(crate) async fn persist_block_sequence(&self, block: Arc<Block>) -> bool {
        let Some(snapshot) = self.system.store_snapshot() else {
            debug!(
                target: "neo",
                index = block.index(),
                "persist_block_sequence: no store snapshot exposed; skipping durable native persistence for store-less context"
            );
            return true;
        };
        let settings = self.system.settings();
        match crate::native_persist::persist_block_natives(
            Arc::clone(&snapshot),
            Arc::clone(&block),
            settings.as_ref(),
        ) {
            Ok(outcome) => {
                if !apply_state_root_changes(self.system.as_ref(), block.index(), &snapshot) {
                    return false;
                }

                debug!(
                    target: "neo",
                    initialized = ?outcome.initialized,
                    engines = outcome.application_executed.len(),
                    "block persistence pipeline completed"
                );
                true
            }
            Err(err) => {
                error!(target: "neo", %err, "block persistence pipeline failed");
                false
            }
        }
    }

    /// Drain the unverified block cache, persisting up to one bounded batch of
    /// consecutive blocks whose parents have landed.
    pub(crate) async fn handle_drain_unverified_blocks(&self) -> usize {
        let mut drained = 0usize;
        while drained < DRAIN_BATCH_SIZE {
            let Some(candidate) = self.pop_next_unverified_block() else {
                break;
            };
            let index = candidate.block.index();
            match self
                .persist_next_expected_block(
                    candidate.block,
                    candidate.relay,
                    candidate.pre_verified,
                )
                .await
            {
                Ok(()) => drained += 1,
                Err(err) => {
                    warn!(
                        target: "neo",
                        index,
                        %err,
                        "dropping parked block that failed verification/persistence"
                    );
                }
            }
        }
        drained
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::{DataCache, StorageItem, StorageKey};

    #[test]
    fn constants_have_expected_values() {
        // Sanity check: the drain batch size is bounded to keep cache pressure
        // predictable.
        assert!(DRAIN_BATCH_SIZE > 0);
        assert!(MAX_UNVERIFIED_CACHE_SIZE >= DRAIN_BATCH_SIZE);
    }

    #[test]
    fn state_root_changes_filter_ledger_and_project_track_states() {
        let snapshot = DataCache::new(false);
        let changed_key = StorageKey::new(5, vec![0xAA]);
        let deleted_key = StorageKey::new(6, vec![0xBB]);
        let ledger_key = StorageKey::new(LEDGER_CONTRACT_ID, vec![0xCC]);

        snapshot.add(deleted_key.clone(), StorageItem::from_bytes(vec![0x00]));
        snapshot.commit();
        snapshot.add(changed_key.clone(), StorageItem::from_bytes(vec![0x01]));
        snapshot.delete(&deleted_key);
        snapshot.add(ledger_key, StorageItem::from_bytes(vec![0x02]));

        let changes = state_root_changes(&snapshot);
        assert_eq!(changes.len(), 2);
        assert!(changes.contains(&MptChange::Put {
            key: changed_key.to_array(),
            value: vec![0x01],
        }));
        assert!(changes.contains(&MptChange::Delete {
            key: deleted_key.to_array(),
        }));
    }
}
