//! Version-aware pruning of Ledger rows already durable in static files.

use std::collections::BTreeSet;

use neo_error::{CoreError, CoreResult};
use neo_native_contracts::LedgerContract;
use neo_native_contracts::ledger_contract::storage::{
    PREFIX_BLOCK, PREFIX_BLOCK_HASH, PREFIX_CURRENT_BLOCK, PREFIX_TRANSACTION,
};
use neo_static_files::StaticFileProvider;
use neo_storage::persistence::{
    Store, StoreMaintenanceBatch, Table, TableEncode, TableNamespace, TableProvider, U32BeCodec,
};
use neo_storage::{StorageKey, StorageResult};

use super::StaticLedgerArchive;

const PRUNED_THROUGH_KEY: &[u8] = b"neo.ledger.hot-pruned-through.v1";

#[derive(Debug)]
struct HotLedgerPruneWatermarkKeyCodec;

impl TableEncode<()> for HotLedgerPruneWatermarkKeyCodec {
    type Encoded<'a> = &'static [u8];

    fn encode(_: &()) -> StorageResult<Self::Encoded<'_>> {
        Ok(PRUNED_THROUGH_KEY)
    }
}

#[derive(Debug)]
struct HotLedgerPruneWatermarkTable;

impl Table for HotLedgerPruneWatermarkTable {
    type Key = ();
    type Value = u32;
    type KeyCodec = HotLedgerPruneWatermarkKeyCodec;
    type ValueCodec = U32BeCodec;

    const NAME: &'static str = "HotLedgerPruneWatermark";
    const NAMESPACE: TableNamespace = TableNamespace::Maintenance;
}

/// Result of one bounded hot-Ledger pruning transaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HotLedgerPruneOutcome {
    /// Watermark before this transaction, or `None` before the first prune.
    pub previous_watermark: Option<u32>,
    /// Watermark durably committed with the row deletions.
    pub pruned_through: Option<u32>,
    /// Archive frames covered by this transaction.
    pub processed_frames: u32,
    /// Hot Ledger rows deleted by this transaction.
    pub deleted_rows: u64,
}

impl StaticLedgerArchive {
    /// Reads and strictly decodes the node-local hot-pruning watermark.
    pub fn hot_pruned_through<S: Store>(&self, store: &S) -> CoreResult<Option<u32>> {
        store
            .table_get::<HotLedgerPruneWatermarkTable>(&())
            .map_err(|error| table_read_error("read hot Ledger prune watermark", error))
    }

    /// Prunes at most `max_frames` archived heights through `target`.
    ///
    /// A row is deleted only when its latest archived version is no newer than
    /// the bounded transaction frontier and the hot and cold bytes match. Row
    /// deletions and watermark advancement share one backend transaction.
    pub fn prune_hot_through<S: Store>(
        &self,
        store: &S,
        target: u32,
        max_frames: usize,
    ) -> CoreResult<HotLedgerPruneOutcome> {
        if max_frames == 0 {
            return Err(CoreError::invalid_operation(
                "hot Ledger prune batch must include at least one frame",
            ));
        }
        let archive_tip = self.tip().ok_or_else(|| {
            CoreError::invalid_operation("cannot prune hot Ledger rows without an archive")
        })?;
        if target > archive_tip {
            return Err(CoreError::invalid_operation(format!(
                "hot Ledger prune target {target} exceeds archive tip {archive_tip}"
            )));
        }

        let previous_watermark = self.hot_pruned_through(store)?;
        if let Some(watermark) = previous_watermark {
            if watermark > archive_tip {
                return Err(CoreError::invalid_data(format!(
                    "hot Ledger prune watermark {watermark} exceeds archive tip {archive_tip}"
                )));
            }
        }
        if previous_watermark.is_some_and(|watermark| watermark >= target) {
            return Ok(HotLedgerPruneOutcome {
                previous_watermark,
                pruned_through: previous_watermark,
                ..HotLedgerPruneOutcome::default()
            });
        }

        let start = previous_watermark.map_or(0, |height| height.saturating_add(1));
        let max_offset = u32::try_from(max_frames.saturating_sub(1)).unwrap_or(u32::MAX);
        let frontier = target.min(start.saturating_add(max_offset));
        let mut keys = BTreeSet::new();
        for height in start..=frontier {
            let frame_keys = self
                .files()
                .frame_row_keys(height)
                .map_err(static_file_error("enumerate archived Ledger frame"))?
                .ok_or_else(|| {
                    CoreError::invalid_data(format!(
                        "static Ledger archive is missing frame {height} below tip {archive_tip}"
                    ))
                })?;
            for raw_key in frame_keys {
                validate_prunable_ledger_key(&raw_key)?;
                keys.insert(raw_key);
            }
        }

        let keys = keys.into_iter().collect::<Vec<_>>();
        let latest_heights = self
            .files()
            .latest_heights_for_keys(&keys)
            .map_err(static_file_error("resolve archived Ledger row versions"))?;
        if latest_heights.len() != keys.len() {
            return Err(CoreError::invalid_data(
                "static Ledger archive returned an incomplete latest-version result",
            ));
        }

        let mut batch = StoreMaintenanceBatch::new();
        let mut deleted_rows = 0u64;
        for (raw_key, latest_height) in keys.iter().zip(latest_heights) {
            let latest_height = latest_height.ok_or_else(|| {
                CoreError::invalid_data(format!(
                    "static Ledger archive frame references an unindexed row {:02x?}",
                    raw_key
                ))
            })?;
            if latest_height > frontier {
                continue;
            }

            let cold_value = self
                .files()
                .get(raw_key)
                .map_err(static_file_error("read archived Ledger row"))?
                .ok_or_else(|| {
                    CoreError::invalid_data(format!(
                        "static Ledger archive latest row is unreadable at height {latest_height}: {:02x?}",
                        raw_key
                    ))
                })?;
            let hot_value = store.try_get_bytes(raw_key).ok_or_else(|| {
                CoreError::invalid_data(format!(
                    "hot Ledger row is missing before prune watermark {frontier}: {:02x?}",
                    raw_key
                ))
            })?;
            if hot_value != cold_value {
                return Err(CoreError::invalid_data(format!(
                    "hot/cold Ledger row mismatch before pruning height {latest_height}: {:02x?}",
                    raw_key
                )));
            }
            batch.delete_data(raw_key.clone());
            deleted_rows = deleted_rows.saturating_add(1);
        }
        batch
            .put::<HotLedgerPruneWatermarkTable>(&(), &frontier)
            .map_err(storage_error("encode hot Ledger prune watermark"))?;
        let committed = store
            .try_commit_durable_maintenance(&batch)
            .map_err(storage_error("commit hot Ledger prune batch"))?;
        if !committed {
            return Err(CoreError::invalid_operation(
                "storage backend does not support atomic hot Ledger maintenance",
            ));
        }

        Ok(HotLedgerPruneOutcome {
            previous_watermark,
            pruned_through: Some(frontier),
            processed_frames: frontier.saturating_sub(start).saturating_add(1),
            deleted_rows,
        })
    }
}

fn validate_prunable_ledger_key(raw_key: &[u8]) -> CoreResult<()> {
    let key = StorageKey::from_bytes(raw_key);
    let suffix = key.key();
    let valid_shape = key.id() == LedgerContract::ID
        && match suffix.first().copied() {
            Some(PREFIX_BLOCK_HASH) => suffix.len() == 5,
            Some(PREFIX_BLOCK) => suffix.len() == 33,
            Some(PREFIX_TRANSACTION) => matches!(suffix.len(), 33 | 53),
            Some(PREFIX_CURRENT_BLOCK) => false,
            _ => false,
        };
    if !valid_shape {
        return Err(CoreError::invalid_data(format!(
            "static Ledger archive contains a non-prunable key: {:02x?}",
            raw_key
        )));
    }
    Ok(())
}

fn storage_error(context: &'static str) -> impl FnOnce(neo_storage::StorageError) -> CoreError {
    move |error| CoreError::io(format!("{context}: {error}"))
}

fn table_read_error(context: &'static str, error: neo_storage::StorageError) -> CoreError {
    match error {
        neo_storage::StorageError::Serialization { .. } => {
            CoreError::invalid_data(format!("{context}: {error}"))
        }
        _ => CoreError::io(format!("{context}: {error}")),
    }
}

fn static_file_error(
    context: &'static str,
) -> impl FnOnce(neo_static_files::StaticFileError) -> CoreError {
    move |error| CoreError::io(format!("{context}: {error}"))
}
