//! Persistence-aware indexer mutation and rollback mechanics.

use super::IndexerService;
use super::backend::PersistenceBackend;
use super::persistence::{MutationPersistenceMode, PendingPersistence};
use crate::error::IndexerResult;
use crate::indexer::Indexer;
use crate::model::IndexerSnapshot;

impl IndexerService {
    pub(super) fn persistence_guard(&self) -> Option<parking_lot::MutexGuard<'_, ()>> {
        self.persistence.as_ref().map(|_| self.persist_lock.lock())
    }

    fn snapshot_for_persistence(&self, indexer: &Indexer) -> Option<IndexerSnapshot> {
        self.persistence.as_ref().map(|_| indexer.snapshot())
    }

    fn persistence_mode_for_mutation(&self) -> MutationPersistenceMode {
        self.persistence.as_deref().map_or(
            MutationPersistenceMode::None,
            PersistenceBackend::mutation_mode,
        )
    }

    pub(super) fn mutate_indexer<T>(
        &self,
        mutate: impl FnOnce(&mut Indexer) -> IndexerResult<(T, bool)>,
    ) -> IndexerResult<T> {
        let _persist_guard = self.persistence_guard();
        let mode = self.persistence_mode_for_mutation();
        let (result, change, rollback_snapshot) = {
            let mut indexer = self.inner.write();
            let rollback_snapshot = if mode.is_persistent() {
                Some(indexer.snapshot())
            } else {
                None
            };
            let (result, should_persist) = mutate(&mut indexer)?;
            let change = if should_persist {
                match mode {
                    MutationPersistenceMode::None => None,
                    MutationPersistenceMode::JsonFile => self
                        .snapshot_for_persistence(&indexer)
                        .map(PendingPersistence::JsonSnapshot),
                    MutationPersistenceMode::Store => {
                        let previous = rollback_snapshot.clone().ok_or(
                            crate::IndexerError::MissingRollbackSnapshot {
                                mode: "service-store",
                            },
                        )?;
                        Some(PendingPersistence::StoreDelta {
                            previous,
                            current: indexer.snapshot(),
                        })
                    }
                }
            } else {
                None
            };
            (result, change, rollback_snapshot)
        };
        let durability_pending = change.is_some();
        if let Err(err) = self.persist_change(change) {
            if let Some(snapshot) = rollback_snapshot {
                self.restore_indexer_after_persistence_failure(snapshot);
            }
            return Err(err);
        }
        if durability_pending {
            self.durability_pending
                .store(true, std::sync::atomic::Ordering::Release);
        }
        Ok(result)
    }

    fn persist_change(&self, change: Option<PendingPersistence>) -> IndexerResult<()> {
        match (self.persistence.as_deref(), change) {
            (Some(backend), Some(change)) => backend.persist_change(change),
            _ => Ok(()),
        }
    }

    fn restore_indexer_after_persistence_failure(&self, snapshot: IndexerSnapshot) {
        match Indexer::from_snapshot(snapshot) {
            Ok(indexer) => {
                *self.inner.write() = indexer;
            }
            Err(err) => {
                tracing::error!(
                    target: "neo::indexer",
                    error = %err,
                    "failed to roll back in-memory indexer after persistence failure"
                );
            }
        }
    }
}
