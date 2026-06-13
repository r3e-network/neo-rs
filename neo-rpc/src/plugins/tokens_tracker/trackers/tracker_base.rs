//! Shared base for token trackers.
//!
//! Provides common functionality for NEP-11 and NEP-17 trackers including
//! database operations and transfer record extraction.

use super::token_transfer_key::TokenTransferKey;
use neo_io::extensions::serializable::SerializableExtensions;
use neo_io::{MemoryReader, Serializable};
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_primitives::{LogLevel, UInt160};
use neo_storage::persistence::{DataCache, SeekDirection, Store, StoreSnapshot};
use neo_system::Node;
use neo_vm::StackItem;
use num_bigint::BigInt;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Transfer record extracted from a `Transfer` notification.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferRecord {
    /// Contract script hash (asset).
    pub asset: UInt160,
    /// Sender address (zero for mint).
    pub from: UInt160,
    /// Recipient address (zero for burn).
    pub to: UInt160,
    /// Token ID for NEP-11 transfers.
    pub token_id: Option<Vec<u8>>,
    /// Transfer amount.
    pub amount: BigInt,
}

/// Common view over transfer-key types (TokenTransferKey, Nep17TransferKey, Nep11TransferKey).
pub trait TokenTransferKeyView {
    /// Returns the user's script hash.
    fn user_script_hash(&self) -> &UInt160;
    /// Returns the timestamp in milliseconds.
    fn timestamp_ms(&self) -> u64;
    /// Returns the asset's script hash.
    fn asset_script_hash(&self) -> &UInt160;
    /// Returns the notification index within the block.
    fn block_xfer_notification_index(&self) -> u32;
}

impl<T> TokenTransferKeyView for T
where
    T: AsRef<TokenTransferKey>,
{
    fn user_script_hash(&self) -> &UInt160 {
        &self.as_ref().user_script_hash
    }

    fn timestamp_ms(&self) -> u64 {
        self.as_ref().timestamp_ms
    }

    fn asset_script_hash(&self) -> &UInt160 {
        &self.as_ref().asset_script_hash
    }

    fn block_xfer_notification_index(&self) -> u32 {
        self.as_ref().block_xfer_notification_index
    }
}

/// Trait implemented by NEP-11/NEP-17 trackers.
pub trait Tracker: Send + Sync {
    /// Returns the tracker's name for logging.
    fn track_name(&self) -> &str;

    /// Called when a block is being persisted.
    fn on_persist(
        &mut self,
        system: &Node,
        block: &Block,
        snapshot: &DataCache,
        executed_list: &[ApplicationExecuted],
    );

    /// Resets the current batch (starts a new snapshot).
    fn reset_batch(&mut self);

    /// Commits the current batch to the database.
    fn commit(&mut self) -> Result<(), String>;
}

/// Base tracker state shared by NEP-11 and NEP-17 trackers.
pub struct TrackerBase {
    /// Whether to track transfer history.
    pub should_track_history: bool,
    /// Maximum results for queries.
    pub max_results: u32,
    /// Database store.
    pub db: Arc<dyn Store>,
    /// Current snapshot for batch operations.
    snapshot: Option<Arc<dyn StoreSnapshot>>,
    /// Reference to the Neo system.
    pub neo_system: Arc<Node>,
}

impl TrackerBase {
    /// Creates a new TrackerBase.
    pub fn new(
        db: Arc<dyn Store>,
        max_results: u32,
        should_track_history: bool,
        neo_system: Arc<Node>,
    ) -> Self {
        Self {
            should_track_history,
            max_results,
            db,
            snapshot: None,
            neo_system,
        }
    }

    /// Resets the batch by creating a new snapshot.
    pub fn reset_batch(&mut self) {
        self.snapshot = Some(self.db.snapshot());
    }

    /// Commits the current snapshot to the database.
    pub fn commit(&mut self) -> Result<(), String> {
        if let Some(snapshot_arc) = self.snapshot.as_mut() {
            if let Some(snapshot) = Arc::get_mut(snapshot_arc) {
                snapshot
                    .try_commit()
                    .map_err(|e| format!("snapshot commit failed: {}", e))?;
            } else {
                return Err("snapshot commit failed: snapshot is still shared".to_string());
            }
        }
        Ok(())
    }

    fn key<K: Serializable>(prefix: u8, key: &K) -> Result<Vec<u8>, String> {
        let mut buffer = Vec::with_capacity(key.size() + 1);
        buffer.push(prefix);
        buffer.extend_from_slice(&key.to_array().map_err(|e| e.to_string())?);
        Ok(buffer)
    }

    /// Stores a key-value pair with the given prefix.
    pub fn put<K: Serializable, V: Serializable>(
        &mut self,
        prefix: u8,
        key: &K,
        value: &V,
    ) -> Result<(), String> {
        let Some(snapshot_arc) = self.snapshot.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Ok(());
        };
        let key_bytes = Self::key(prefix, key)?;
        let value_bytes = value.to_array().map_err(|e| e.to_string())?;
        snapshot
            .put(key_bytes, value_bytes)
            .map_err(|e| format!("storage put failed: {}", e))?;
        Ok(())
    }

    /// Deletes a key with the given prefix.
    pub fn delete<K: Serializable>(&mut self, prefix: u8, key: &K) -> Result<(), String> {
        let Some(snapshot_arc) = self.snapshot.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Ok(());
        };
        let key_bytes = Self::key(prefix, key)?;
        snapshot
            .delete(key_bytes)
            .map_err(|e| format!("storage delete failed: {}", e))?;
        Ok(())
    }

    /// Queries transfers within a time range.
    pub fn query_transfers<TKey, TValue>(
        &self,
        db_prefix: u8,
        user_script_hash: &UInt160,
        start_time: u64,
        end_time: u64,
    ) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: Serializable,
        TValue: Serializable,
    {
        let mut prefix_bytes = vec![db_prefix];
        prefix_bytes.extend_from_slice(&user_script_hash.to_bytes());

        let start_key = [prefix_bytes.as_slice(), &start_time.to_be_bytes()].concat();
        let end_key = [prefix_bytes.as_slice(), &end_time.to_be_bytes()].concat();

        let start_vec = start_key.clone();
        let mut results = Vec::new();

        let snapshot = self.db.snapshot();
        for (key_bytes, value_bytes) in snapshot.find(Some(&start_vec), SeekDirection::Forward) {
            if key_bytes.as_slice() > end_key.as_slice() {
                break;
            }
            if !key_bytes.starts_with(&prefix_bytes) {
                break;
            }

            let mut key_reader = MemoryReader::new(&key_bytes[1..]);
            let key = TKey::deserialize(&mut key_reader).map_err(|e| e.to_string())?;

            let mut val_reader = MemoryReader::new(&value_bytes);
            let val = TValue::deserialize(&mut val_reader).map_err(|e| e.to_string())?;

            results.push((key, val));
        }
        Ok(results)
    }

    /// Extracts a transfer record from a `Transfer` notification payload.
    pub fn get_transfer_record(
        asset: &UInt160,
        state_items: &[StackItem],
    ) -> Option<TransferRecord> {
        if state_items.len() < 3 {
            return None;
        }

        let from_item = &state_items[0];
        let to_item = &state_items[1];
        let amount_item = &state_items[2];

        if !from_item.is_null() && !matches!(from_item, StackItem::ByteString(_)) {
            return None;
        }
        if !to_item.is_null() && !matches!(to_item, StackItem::ByteString(_)) {
            return None;
        }
        if !matches!(
            amount_item,
            StackItem::ByteString(_) | StackItem::Integer(_)
        ) {
            return None;
        }

        let from_bytes = if from_item.is_null() {
            None
        } else {
            Some(from_item.as_bytes().ok()?)
        };
        if let Some(ref bytes) = from_bytes {
            if bytes.len() != 20 {
                return None;
            }
        }

        let to_bytes = if to_item.is_null() {
            None
        } else {
            Some(to_item.as_bytes().ok()?)
        };
        if let Some(ref bytes) = to_bytes {
            if bytes.len() != 20 {
                return None;
            }
        }

        if from_bytes.is_none() && to_bytes.is_none() {
            return None;
        }

        let from = from_bytes
            .as_ref()
            .and_then(|b| UInt160::from_bytes(b).ok())
            .unwrap_or_else(UInt160::zero);
        let to = to_bytes
            .as_ref()
            .and_then(|b| UInt160::from_bytes(b).ok())
            .unwrap_or_else(UInt160::zero);

        let amount = amount_item.as_integer().ok()?;

        let token_id = if state_items.len() == 4 {
            match &state_items[3] {
                StackItem::ByteString(bytes) => Some(bytes.clone()),
                _ => None,
            }
        } else {
            None
        };

        Some(TransferRecord {
            asset: *asset,
            from,
            to,
            token_id,
            amount,
        })
    }

    /// Base JSON conversion for a transfer pair (without NEP-11 tokenid).
    pub fn transfer_to_json<K: TokenTransferKeyView>(
        &self,
        key: &K,
        value: &super::token_transfer::TokenTransfer,
    ) -> Value {
        let transfer_address = if value.user_script_hash == UInt160::zero() {
            Value::Null
        } else {
            Value::String(value.user_script_hash.to_address())
        };
        json!({
            "timestamp": key.timestamp_ms(),
            "assethash": key.asset_script_hash().to_string(),
            "transferaddress": transfer_address,
            "amount": value.amount.to_string(),
            "blockindex": value.block_index,
            "transfernotifyindex": key.block_xfer_notification_index(),
            "txhash": value.tx_hash.to_string(),
        })
    }

    /// Logs a message with the tracker name.
    pub fn log(track: &str, message: &str, level: LogLevel) {
        match level {
            LogLevel::Debug => debug!(target: "neo::tokens_tracker", track, message),
            LogLevel::Info => info!(target: "neo::tokens_tracker", track, message),
            LogLevel::Warning => warn!(target: "neo::tokens_tracker", track, message),
            LogLevel::Error | LogLevel::Fatal => {
                error!(target: "neo::tokens_tracker", track, message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_storage::persistence::{
        read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
        storage::StorageError,
        store::OnNewSnapshotDelegate,
        write_store::WriteStore,
    };
    use neo_storage::{StorageItem, StorageKey};
    use std::any::Any;

    #[derive(Clone)]
    struct FailingStore;

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingStore {
        fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&Vec<u8>>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FailingStore {
        fn try_get(&self, _key: &StorageKey) -> Option<StorageItem> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&StorageKey>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl ReadOnlyStore for FailingStore {}

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingStore {
        fn delete(&mut self, _key: Vec<u8>) -> neo_storage::StorageResult<()> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> neo_storage::StorageResult<()> {
            Ok(())
        }
    }

    impl Store for FailingStore {
        fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
            Arc::new(FailingSnapshot {
                store: Arc::new(self.clone()),
            })
        }

        fn on_new_snapshot(&self, _handler: OnNewSnapshotDelegate) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct FailingSnapshot {
        store: Arc<dyn Store>,
    }

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&Vec<u8>>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn delete(&mut self, _key: Vec<u8>) -> neo_storage::StorageResult<()> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> neo_storage::StorageResult<()> {
            Ok(())
        }
    }

    impl StoreSnapshot for FailingSnapshot {
        fn store(&self) -> Arc<dyn Store> {
            Arc::clone(&self.store)
        }

        fn try_commit(&mut self) -> neo_storage::persistence::store_snapshot::SnapshotCommitResult {
            Err(StorageError::CommitFailed(
                "injected tracker commit failure".to_string(),
            ))
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tracker_base_commit_propagates_snapshot_try_commit_failure() {
        let system =
            Node::new(Arc::new(ProtocolSettings::mainnet()), None, None).expect("neo system");
        let mut tracker = TrackerBase::new(Arc::new(FailingStore), 100, true, Arc::new(system));
        tracker.reset_batch();

        let err = tracker
            .commit()
            .expect_err("tracker commit should propagate snapshot commit failure");

        assert!(err.contains("injected tracker commit failure"));
    }
}
