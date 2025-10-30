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
    store_get: Option<Arc<dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync>>,
    store_find: Option<
        Arc<
            dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)>
                + Send
                + Sync,
        >,
    >,
}

impl Clone for DataCache {
    fn clone(&self) -> Self {
        Self {
            dictionary: Arc::clone(&self.dictionary),
            change_set: self.change_set.as_ref().map(Arc::clone),
            on_read: Arc::clone(&self.on_read),
            on_update: Arc::clone(&self.on_update),
            store_get: self.store_get.as_ref().map(Arc::clone),
            store_find: self.store_find.as_ref().map(Arc::clone),
        }
    }
}

impl DataCache {
    /// Creates a new DataCache.
    pub fn new(read_only: bool) -> Self {
        Self::new_with_store(read_only, None, None)
    }

    /// Creates a new DataCache with an optional backing store.
    pub fn new_with_store(
        read_only: bool,
        store_get: Option<Arc<dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync>>,
        store_find: Option<
            Arc<
                dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)>
                    + Send
                    + Sync,
            >,
        >,
    ) -> Self {
        Self {
            dictionary: Arc::new(RwLock::new(HashMap::new())),
            change_set: if read_only {
                None
            } else {
                Some(Arc::new(RwLock::new(HashSet::new())))
            },
            on_read: Arc::new(RwLock::new(Vec::new())),
            on_update: Arc::new(RwLock::new(Vec::new())),
            store_get,
            store_find,
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
        if let Some(trackable) = self.dictionary.read().unwrap().get(key) {
            if trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound {
                return Some(trackable.item.clone());
            }
            return None;
        }

        if let Some(getter) = &self.store_get {
            if let Some(item) = getter(key) {
                {
                    let mut dict = self.dictionary.write().unwrap();
                    dict.entry(key.clone())
                        .or_insert_with(|| Trackable::new(item.clone(), TrackState::None));
                }

                let handlers = self.on_read.read().unwrap();
                for handler in handlers.iter() {
                    handler(self, key, &item);
                }

                return Some(item);
            }
        }

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

    /// Returns a snapshot of all tracked entries, typically used when
    /// propagating changes into an underlying store.
    pub fn tracked_items(&self) -> Vec<(StorageKey, Trackable)> {
        let dict = self.dictionary.read().unwrap();
        if let Some(change_set) = &self.change_set {
            let keys: Vec<_> = change_set.read().unwrap().iter().cloned().collect();
            keys.into_iter()
                .filter_map(|key| dict.get(&key).cloned().map(|track| (key, track)))
                .collect()
        } else {
            dict.iter()
                .map(|(key, track)| (key.clone(), track.clone()))
                .collect()
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
        let mut combined: HashMap<StorageKey, StorageItem> = HashMap::new();

        for (key, trackable) in self.dictionary.read().unwrap().iter() {
            if trackable.state == TrackState::Deleted || trackable.state == TrackState::NotFound {
                continue;
            }

            if let Some(prefix) = key_prefix {
                if key.id != prefix.id || !key.suffix().starts_with(prefix.suffix()) {
                    continue;
                }
            }

            combined
                .entry(key.clone())
                .or_insert_with(|| trackable.item.clone());
        }

        if let Some(finder) = &self.store_find {
            for (key, value) in finder(key_prefix, SeekDirection::Forward) {
                combined.entry(key).or_insert(value);
            }
        }

        let mut items: Vec<_> = combined.into_iter().collect();
        items.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));

        if direction == SeekDirection::Backward {
            Box::new(items.into_iter().rev())
        } else {
            Box::new(items.into_iter())
        }
    }
}
