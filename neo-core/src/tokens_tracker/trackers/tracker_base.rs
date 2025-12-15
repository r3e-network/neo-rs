//! Shared base for token trackers.
//!
//! Provides common functionality for NEP-11 and NEP-17 trackers including
//! database operations and transfer record extraction.

use crate::extensions::log_level::LogLevel;
use crate::neo_io::{MemoryReader, Serializable, SerializableExt};
use crate::neo_ledger::{ApplicationExecuted, Block};
use crate::persistence::{DataCache, IStore, IStoreSnapshot, SeekDirection};
use crate::{NeoSystem, UInt160};
use neo_vm::stack_item::StackItem;
use num_bigint::BigInt;
use serde_json::{json, Value};
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

/// Trait implemented by NEP-11/NEP-17 trackers.
pub trait Tracker: Send + Sync {
    /// Returns the tracker's name for logging.
    fn track_name(&self) -> &str;

    /// Called when a block is being persisted.
    fn on_persist(
        &mut self,
        system: &NeoSystem,
        block: &Block,
        snapshot: &DataCache,
        executed_list: &[ApplicationExecuted],
    );

    /// Resets the current batch (starts a new snapshot).
    fn reset_batch(&mut self);

    /// Commits the current batch to the database.
    fn commit(&mut self);
}

/// Base tracker state shared by NEP-11 and NEP-17 trackers.
pub struct TrackerBase {
    /// Whether to track transfer history.
    pub should_track_history: bool,
    /// Maximum results for queries.
    pub max_results: u32,
    /// Database store.
    pub db: Arc<dyn IStore>,
    /// Current snapshot for batch operations.
    snapshot: Option<Arc<dyn IStoreSnapshot>>,
    /// Reference to the Neo system.
    pub neo_system: Arc<NeoSystem>,
}

impl TrackerBase {
    /// Creates a new TrackerBase.
    pub fn new(
        db: Arc<dyn IStore>,
        max_results: u32,
        should_track_history: bool,
        neo_system: Arc<NeoSystem>,
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
        self.snapshot = Some(self.db.get_snapshot());
    }

    /// Commits the current snapshot to the database.
    pub fn commit(&mut self) {
        if let Some(snapshot_arc) = self.snapshot.as_mut() {
            if let Some(snapshot) = Arc::get_mut(snapshot_arc) {
                snapshot.commit();
            }
        }
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
        snapshot.put(key_bytes, value_bytes);
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
        snapshot.delete(key_bytes);
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

        let snapshot = self.db.get_snapshot();
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

        let amount = amount_item.get_integer().ok()?;

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
