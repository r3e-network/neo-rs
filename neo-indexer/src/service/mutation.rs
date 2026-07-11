//! Persistence-aware indexer mutation and rollback mechanics.

use super::IndexerService;
use crate::error::IndexerResult;
use crate::indexer::{Indexer, PreparedIndexBatch};
use crate::model::{BlockIndexRecord, IndexerSnapshot};

impl IndexerService {
    pub(super) fn persistence_guard(&self) -> Option<parking_lot::MutexGuard<'_, ()>> {
        self.persistence.as_ref().map(|_| self.persist_lock.lock())
    }

    pub(super) fn mutate_indexer<T>(
        &self,
        mutate: impl FnOnce(&mut Indexer) -> IndexerResult<(T, bool)>,
    ) -> IndexerResult<T> {
        let _persist_guard = self.persistence_guard();
        let is_persistent = self.persistence.is_some();
        let mut indexer = self.inner.write();
        let rollback_snapshot = if is_persistent {
            Some(indexer.snapshot())
        } else {
            None
        };
        let (result, should_persist) = mutate(&mut indexer)?;
        let change = if should_persist && is_persistent {
            rollback_snapshot
                .clone()
                .map(|previous| (previous, indexer.snapshot()))
        } else {
            None
        };
        let durability_pending = change.is_some();
        if let Err(err) = self.persist_change(change) {
            if let Some(snapshot) = rollback_snapshot {
                match Indexer::from_snapshot(snapshot) {
                    Ok(previous) => *indexer = previous,
                    Err(rollback_error) => {
                        tracing::error!(
                            target: "neo::indexer",
                            error = %rollback_error,
                            "failed to restore in-memory indexer after persistence failure"
                        );
                    }
                }
            }
            return Err(err);
        }
        drop(indexer);
        if durability_pending {
            self.durability_pending
                .store(true, std::sync::atomic::Ordering::Release);
        }
        Ok(result)
    }

    pub(super) fn commit_prepared_batch(
        &self,
        prepared: PreparedIndexBatch,
    ) -> IndexerResult<Vec<BlockIndexRecord>> {
        if prepared.is_empty() {
            return Ok(Vec::new());
        }

        let _persist_guard = self.persistence_guard();
        let mut indexer = self.inner.write();
        let change = indexer.projection_change_set(&prepared)?;
        if let Some(backend) = self.persistence.as_deref() {
            backend.persist_projection_change(&change)?;
        }
        let records = indexer.apply_prepared_batch(prepared);
        drop(indexer);

        if self.persistence.is_some() && !change.is_empty() {
            self.durability_pending
                .store(true, std::sync::atomic::Ordering::Release);
        }
        Ok(records)
    }

    fn persist_change(
        &self,
        change: Option<(IndexerSnapshot, IndexerSnapshot)>,
    ) -> IndexerResult<()> {
        match (self.persistence.as_deref(), change) {
            (Some(backend), Some((previous, current))) => {
                backend.persist_change(&previous, &current)
            }
            _ => Ok(()),
        }
    }
}
