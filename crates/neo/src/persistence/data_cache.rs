// Copyright (C) 2015-2025 The Neo Project.
//
// data_cache.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    track_state::TrackState,
};
use crate::smart_contract::{StorageItem, StorageKey};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Represents an entry in the cache.
#[derive(Debug, Clone)]
pub struct Trackable {
    /// The data of the entry.
    pub item: StorageItem,

    /// The state of the entry.
    pub state: TrackState,
}

impl Trackable {
    /// Creates a new Trackable.
    pub fn new(item: StorageItem, state: TrackState) -> Self {
        Self { item, state }
    }
}

/// Delegate for storage entries
pub type OnEntryDelegate = Box<dyn Fn(&DataCache, &StorageKey, &StorageItem) + Send + Sync>;

/// Represents a cache for the underlying storage of the NEO blockchain.
pub struct DataCache {
    dictionary: Arc<RwLock<HashMap<StorageKey, Trackable>>>,
    change_set: Option<Arc<RwLock<HashSet<StorageKey>>>>,
    on_read: Arc<RwLock<Vec<OnEntryDelegate>>>,
    on_update: Arc<RwLock<Vec<OnEntryDelegate>>>,
}

impl DataCache {
    /// Creates a new DataCache.
    pub fn new(read_only: bool) -> Self {
        Self {
            dictionary: Arc::new(RwLock::new(HashMap::new())),
            change_set: if read_only {
                None
            } else {
                Some(Arc::new(RwLock::new(HashSet::new())))
            },
            on_read: Arc::new(RwLock::new(Vec::new())),
            on_update: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Returns true if DataCache is read-only.
    pub fn is_read_only(&self) -> bool {
        self.change_set.is_none()
    }

    /// Adds a handler for read events.
    pub fn on_read(&self, handler: OnEntryDelegate) {
        self.on_read.write().unwrap().push(handler);
    }

    /// Adds a handler for update events.
    pub fn on_update(&self, handler: OnEntryDelegate) {
        self.on_update.write().unwrap().push(handler);
    }

    /// Gets an item from the cache.
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        let dict = self.dictionary.read().unwrap();
        if let Some(trackable) = dict.get(key) {
            if trackable.state == TrackState::Deleted || trackable.state == TrackState::NotFound {
                return None;
            }
            return Some(trackable.item.clone());
        }

        // Would load from underlying storage here
        None
    }

    /// Adds an item to the cache.
    pub fn add(&self, key: StorageKey, value: StorageItem) {
        if self.is_read_only() {
            panic!("Cannot add to read-only cache");
        }

        let trackable = Trackable::new(value.clone(), TrackState::Added);
        self.dictionary
            .write()
            .unwrap()
            .insert(key.clone(), trackable);

        if let Some(ref change_set) = self.change_set {
            change_set.write().unwrap().insert(key.clone());
        }

        // Trigger update event
        let handlers = self.on_update.read().unwrap();
        for handler in handlers.iter() {
            handler(self, &key, &value);
        }
    }

    /// Updates an item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        if self.is_read_only() {
            panic!("Cannot update read-only cache");
        }

        let mut dict = self.dictionary.write().unwrap();
        if let Some(trackable) = dict.get_mut(&key) {
            trackable.item = value.clone();
            if trackable.state == TrackState::None {
                trackable.state = TrackState::Changed;
            }
        } else {
            dict.insert(
                key.clone(),
                Trackable::new(value.clone(), TrackState::Changed),
            );
        }

        if let Some(ref change_set) = self.change_set {
            change_set.write().unwrap().insert(key.clone());
        }

        // Trigger update event
        let handlers = self.on_update.read().unwrap();
        for handler in handlers.iter() {
            handler(self, &key, &value);
        }
    }

    /// Deletes an item from the cache.
    pub fn delete(&self, key: &StorageKey) {
        if self.is_read_only() {
            panic!("Cannot delete from read-only cache");
        }

        let mut dict = self.dictionary.write().unwrap();
        if let Some(trackable) = dict.get_mut(key) {
            trackable.state = TrackState::Deleted;
        } else {
            dict.insert(
                key.clone(),
                Trackable::new(StorageItem::default(), TrackState::Deleted),
            );
        }

        if let Some(ref change_set) = self.change_set {
            change_set.write().unwrap().insert(key.clone());
        }
    }

    /// Commits changes to the underlying storage.
    pub fn commit(&self) {
        if self.is_read_only() {
            panic!("Cannot commit read-only cache");
        }

        // In a real implementation, this would write to the underlying storage
        // For now, we just clear the change set
        if let Some(ref change_set) = self.change_set {
            change_set.write().unwrap().clear();
        }
    }

    /// Gets the change set.
    pub fn get_change_set(&self) -> Vec<StorageKey> {
        if let Some(ref change_set) = self.change_set {
            change_set.read().unwrap().iter().cloned().collect()
        } else {
            Vec::new()
        }
    }
}

impl IReadOnlyStore for DataCache {}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for DataCache {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let dict = self.dictionary.read().unwrap();
        let mut items: Vec<_> = dict
            .iter()
            .filter(|(_, trackable)| {
                trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound
            })
            .filter(|(k, _)| {
                if let Some(prefix) = key_prefix {
                    // Check if key starts with prefix
                    k.id == prefix.id && k.key.starts_with(&prefix.key)
                } else {
                    true
                }
            })
            .map(|(k, trackable)| (k.clone(), trackable.item.clone()))
            .collect();

        items.sort_by(|a, b| a.0.key().cmp(b.0.key()));

        if direction == SeekDirection::Backward {
            Box::new(items.into_iter().rev())
        } else {
            Box::new(items.into_iter())
        }
    }
}
