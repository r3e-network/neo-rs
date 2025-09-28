// Copyright (C) 2015-2025 The Neo Project.
//
// tracker_base.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, NeoSystem, Block, DataCache, ApplicationExecuted, IStore, IStoreSnapshot, ISerializable, StackItem, ByteString, Integer};
use neo_core::io::ISerializable as Serializable;
use serde_json::Value;
use std::collections::HashMap;
use std::num::BigInt;

/// Transfer record for tracking token transfers.
/// Matches C# TransferRecord record
#[derive(Debug, Clone, PartialEq)]
pub struct TransferRecord {
    /// Asset script hash
    pub asset: UInt160,
    /// From address
    pub from: UInt160,
    /// To address
    pub to: UInt160,
    /// Token ID (for NEP-11)
    pub token_id: Option<Vec<u8>>,
    /// Amount transferred
    pub amount: BigInt,
}

impl TransferRecord {
    /// Creates a new TransferRecord instance.
    pub fn new(asset: UInt160, from: UInt160, to: UInt160, token_id: Option<Vec<u8>>, amount: BigInt) -> Self {
        Self {
            asset,
            from,
            to,
            token_id,
            amount,
        }
    }
}

/// Base trait for token trackers.
/// Matches C# TrackerBase abstract class
pub trait TrackerBase: Send + Sync {
    /// Gets the track name.
    /// Matches C# TrackName property
    fn track_name(&self) -> &str;
    
    /// Handles persist events.
    /// Matches C# OnPersist method
    fn on_persist(&mut self, system: &NeoSystem, block: &Block, snapshot: &DataCache, executed_list: &[ApplicationExecuted]);
    
    /// Resets the batch.
    /// Matches C# ResetBatch method
    fn reset_batch(&mut self);
    
    /// Commits the batch.
    /// Matches C# Commit method
    fn commit(&mut self);
    
    /// Queries transfers.
    /// Matches C# QueryTransfers method
    fn query_transfers<TKey, TValue>(&self, db_prefix: u8, user_script_hash: UInt160, start_time: u64, end_time: u64) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: Serializable + Default,
        TValue: Serializable + Default;
    
    /// Gets a transfer record from state items.
    /// Matches C# GetTransferRecord method
    fn get_transfer_record(&self, asset: UInt160, state_items: &[StackItem]) -> Option<TransferRecord>;
    
    /// Converts to JSON.
    /// Matches C# ToJson method
    fn to_json(&self, key: &dyn Serializable, value: &dyn Serializable) -> Value;
    
    /// Logs a message.
    /// Matches C# Log method
    fn log(&self, message: &str, level: LogLevel);
}

/// Log level enumeration.
/// Matches C# LogLevel enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Debug level
    Debug,
    /// Info level
    Info,
    /// Warning level
    Warning,
    /// Error level
    Error,
}

/// Base implementation for token trackers.
/// Matches C# TrackerBase abstract class
pub struct BaseTracker {
    /// Whether to track history
    /// Matches C# _shouldTrackHistory field
    should_track_history: bool,
    
    /// Maximum results
    /// Matches C# _maxResults field
    max_results: u32,
    
    /// Database store
    /// Matches C# _db field
    db: Arc<dyn IStore>,
    
    /// Database snapshot
    /// Matches C# _levelDbSnapshot field
    level_db_snapshot: Option<Arc<dyn IStoreSnapshot>>,
    
    /// Neo system reference
    /// Matches C# _neoSystem field
    neo_system: Arc<NeoSystem>,
}

impl BaseTracker {
    /// Creates a new BaseTracker instance.
    /// Matches C# constructor
    pub fn new(db: Arc<dyn IStore>, max_results: u32, should_track_history: bool, neo_system: Arc<NeoSystem>) -> Self {
        Self {
            should_track_history,
            max_results,
            db,
            level_db_snapshot: None,
            neo_system,
        }
    }
    
    /// Gets the track name.
    /// Matches C# TrackName property
    pub fn track_name(&self) -> &str {
        "BaseTracker"
    }
    
    /// Resets the batch.
    /// Matches C# ResetBatch method
    pub fn reset_batch(&mut self) {
        self.level_db_snapshot = Some(self.db.get_snapshot());
    }
    
    /// Commits the batch.
    /// Matches C# Commit method
    pub fn commit(&mut self) {
        if let Some(snapshot) = &self.level_db_snapshot {
            snapshot.commit();
        }
    }
    
    /// Queries transfers.
    /// Matches C# QueryTransfers method
    pub fn query_transfers<TKey, TValue>(&self, db_prefix: u8, user_script_hash: UInt160, start_time: u64, end_time: u64) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: Serializable + Default,
        TValue: Serializable + Default,
    {
        let mut prefix = vec![db_prefix];
        prefix.extend_from_slice(&user_script_hash.to_bytes());
        
        let start_time_bytes = start_time.to_be_bytes();
        let end_time_bytes = end_time.to_be_bytes();
        
        let start_key = [prefix.as_slice(), &start_time_bytes].concat();
        let end_key = [prefix.as_slice(), &end_time_bytes].concat();
        
        self.db.find_range(&start_key, &end_key)
    }
    
    /// Creates a key with prefix.
    /// Matches C# Key method
    pub fn create_key(prefix: u8, key: &dyn Serializable) -> Vec<u8> {
        let mut buffer = vec![0u8; key.size() + 1];
        buffer[0] = prefix;
        key.serialize(&mut buffer[1..].as_mut())?;
        Ok(buffer)
    }
    
    /// Puts a value in the database.
    /// Matches C# Put method
    pub fn put(&mut self, prefix: u8, key: &dyn Serializable, value: &dyn Serializable) -> Result<(), String> {
        if let Some(snapshot) = &self.level_db_snapshot {
            let key_bytes = Self::create_key(prefix, key)?;
            let value_bytes = value.serialize_to_bytes()?;
            snapshot.put(&key_bytes, &value_bytes)?;
        }
        Ok(())
    }
    
    /// Deletes a value from the database.
    /// Matches C# Delete method
    pub fn delete(&mut self, prefix: u8, key: &dyn Serializable) -> Result<(), String> {
        if let Some(snapshot) = &self.level_db_snapshot {
            let key_bytes = Self::create_key(prefix, key)?;
            snapshot.delete(&key_bytes)?;
        }
        Ok(())
    }
    
    /// Gets a transfer record from state items.
    /// Matches C# GetTransferRecord method
    pub fn get_transfer_record(&self, asset: UInt160, state_items: &[StackItem]) -> Option<TransferRecord> {
        if state_items.len() < 3 {
            return None;
        }
        
        let from_item = &state_items[0];
        let to_item = &state_items[1];
        let amount_item = &state_items[2];
        
        if from_item.not_null() && !matches!(from_item, StackItem::ByteString(_)) {
            return None;
        }
        
        if to_item.not_null() && !matches!(to_item, StackItem::ByteString(_)) {
            return None;
        }
        
        if !matches!(amount_item, StackItem::ByteString(_) | StackItem::Integer(_)) {
            return None;
        }
        
        let from_bytes = if from_item.is_null() {
            None
        } else {
            Some(from_item.get_bytes())
        };
        
        let to_bytes = if to_item.is_null() {
            None
        } else {
            Some(to_item.get_bytes())
        };
        
        if from_bytes.is_none() && to_bytes.is_none() {
            return None;
        }
        
        let from = from_bytes.map(|b| UInt160::from_bytes(&b)).unwrap_or(UInt160::zero());
        let to = to_bytes.map(|b| UInt160::from_bytes(&b)).unwrap_or(UInt160::zero());
        
        let amount = match amount_item {
            StackItem::Integer(i) => i.clone(),
            StackItem::ByteString(bs) => BigInt::from_bytes_be(num_bigint::Sign::Plus, bs),
            _ => return None,
        };
        
        match state_items.len() {
            3 => Some(TransferRecord::new(asset, from, to, None, amount)),
            4 => {
                if let StackItem::ByteString(token_id) = &state_items[3] {
                    Some(TransferRecord::new(asset, from, to, Some(token_id.clone()), amount))
                } else {
                    None
                }
            },
            _ => None,
        }
    }
    
    /// Converts to JSON.
    /// Matches C# ToJson method
    pub fn to_json(&self, key: &dyn Serializable, value: &dyn Serializable) -> Value {
        // In a real implementation, this would convert to JSON
        serde_json::Value::Object(serde_json::Map::new())
    }
    
    /// Logs a message.
    /// Matches C# Log method
    pub fn log(&self, message: &str, level: LogLevel) {
        println!("[{}] {}: {}", self.track_name(), level, message);
    }
}

impl Drop for BaseTracker {
    fn drop(&mut self) {
        if let Some(snapshot) = &self.level_db_snapshot {
            // Dispose managed resources
            // snapshot.dispose();
        }
    }
}