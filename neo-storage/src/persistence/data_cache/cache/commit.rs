use super::*;
impl PersistenceDataCache {
    /// Attempt to add an item to the cache, returning an error when read-only.
    pub fn try_add(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.ensure_writable()?;
        self.add_writable(key, value)
    }

    /// Attempt to update an item in the cache, returning an error when read-only.
    pub fn try_update(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.ensure_writable()?;
        self.update_writable(key, value);
        Ok(())
    }

    /// Attempt to delete an item in the cache, returning an error when read-only.
    pub fn try_delete(&self, key: &StorageKey) -> DataCacheResult {
        self.ensure_writable()?;
        self.delete_writable(key);
        Ok(())
    }

    /// Attempts to commit, returning an error when read-only.
    pub fn try_commit(&self) -> DataCacheResult {
        self.ensure_writable()?;
        self.commit_writable();
        Ok(())
    }

    fn ensure_writable(&self) -> DataCacheResult {
        if self.read_only {
            Err(DataCacheError::ReadOnly)
        } else {
            Ok(())
        }
    }

    /// Adds an item to the cache.
    pub fn add(&self, key: StorageKey, value: StorageItem) {
        if self.try_add(key, value).is_err() {
            warn!("attempted to add to read-only PersistenceDataCache");
        }
    }

    fn add_writable(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if let Some(ref cache) = self.hot_key_cache {
            cache.invalidate(&key);
        }
        if let Some(ref cache) = self.read_cache {
            cache.remove(&key);
        }

        let handlers = self.on_update.read();
        if handlers.is_empty() {
            drop(handlers);
            self.apply_add(key, value)?;
        } else {
            let key_clone = key.clone();
            let value_clone = value.clone();
            self.apply_add(key, value)?;
            for handler in handlers.iter() {
                handler(self, &key_clone, &value_clone);
            }
        }
        Ok(())
    }

    fn apply_add(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        use std::collections::hash_map::Entry;
        let mut state = self.state.write();
        match state.dictionary.entry(key) {
            Entry::Occupied(mut occupied) => {
                let prev_state = occupied.get().state;
                let new_state = match prev_state {
                    TrackState::Deleted => TrackState::Changed,
                    TrackState::NotFound => TrackState::Added,
                    TrackState::None | TrackState::Added | TrackState::Changed => {
                        return Err(DataCacheError::InvalidState(format!(
                            "Add is invalid for existing state {prev_state:?}"
                        )));
                    }
                };
                log_watched_storage_event(
                    "add",
                    "apply_add",
                    occupied.key(),
                    Some(prev_state),
                    Some(new_state),
                    Some(&value),
                );
                let entry = occupied.get_mut();
                entry.item = value;
                entry.state = new_state;
            }
            Entry::Vacant(vacant) => {
                log_watched_storage_event(
                    "add",
                    "apply_add",
                    vacant.key(),
                    Some(TrackState::NotFound),
                    Some(TrackState::Added),
                    Some(&value),
                );
                vacant.insert(Trackable::new(value, TrackState::Added));
            }
        }
        Ok(())
    }

    /// Updates an item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        if self.try_update(key, value).is_err() {
            warn!("attempted to update read-only PersistenceDataCache");
        }
    }

    fn update_writable(&self, key: StorageKey, value: StorageItem) {
        if let Some(ref cache) = self.hot_key_cache {
            cache.invalidate(&key);
        }
        if let Some(ref cache) = self.read_cache {
            let size = value.value_bytes().len() + std::mem::size_of::<StorageKey>();
            cache.put(key.clone(), value.clone(), size);
        }

        let handlers = self.on_update.read();
        if handlers.is_empty() {
            drop(handlers);
            self.apply_update(key, value);
        } else {
            let key_clone = key.clone();
            let value_clone = value.clone();
            self.apply_update(key, value);
            for handler in handlers.iter() {
                handler(self, &key_clone, &value_clone);
            }
        }
    }

    fn apply_update(&self, key: StorageKey, value: StorageItem) {
        use std::collections::hash_map::Entry;
        let mut state = self.state.write();
        match state.dictionary.entry(key) {
            Entry::Occupied(mut occupied) => {
                let prev_state = occupied.get().state;
                // A tombstone that is re-populated modifies pre-existing data:
                // tracking it as `Added` would make a later delete cancel the
                // entry entirely and lose the tombstone.
                let new_state = match prev_state {
                    TrackState::Added => TrackState::Added,
                    TrackState::Changed | TrackState::None | TrackState::Deleted => {
                        TrackState::Changed
                    }
                    TrackState::NotFound => TrackState::Changed,
                };
                log_watched_storage_event(
                    "update",
                    "apply_update",
                    occupied.key(),
                    Some(prev_state),
                    Some(new_state),
                    Some(&value),
                );
                let entry = occupied.get_mut();
                entry.item = value;
                entry.state = new_state;
            }
            Entry::Vacant(vacant) => {
                log_watched_storage_event(
                    "update",
                    "apply_update",
                    vacant.key(),
                    Some(TrackState::NotFound),
                    Some(TrackState::Changed),
                    Some(&value),
                );
                vacant.insert(Trackable::new(value, TrackState::Changed));
            }
        }
    }

    /// Deletes an item from the cache.
    pub fn delete(&self, key: &StorageKey) {
        if self.try_delete(key).is_err() {
            warn!("attempted to delete from read-only PersistenceDataCache");
        }
    }

    fn delete_writable(&self, key: &StorageKey) {
        if let Some(ref cache) = self.hot_key_cache {
            cache.invalidate(key);
        }
        if let Some(ref cache) = self.read_cache {
            cache.remove(key);
        }

        self.apply_delete(key);
    }

    fn apply_delete(&self, key: &StorageKey) {
        let prev_state = {
            let state = self.state.read();
            state
                .dictionary
                .get(key)
                .map_or(TrackState::NotFound, |t| t.state)
        };

        match prev_state {
            TrackState::Added => {
                let mut state = self.state.write();
                state.dictionary.remove(key);
                log_watched_storage_event(
                    "delete",
                    "apply_delete_added",
                    key,
                    Some(prev_state),
                    Some(TrackState::NotFound),
                    None,
                );
            }
            TrackState::Changed | TrackState::None => {
                let mut state = self.state.write();
                if let Some(entry) = state.dictionary.get_mut(key) {
                    entry.item = StorageItem::default();
                    entry.state = TrackState::Deleted;
                } else {
                    state.dictionary.insert(
                        key.clone(),
                        Trackable::new(StorageItem::default(), TrackState::Deleted),
                    );
                }
                log_watched_storage_event(
                    "delete",
                    "apply_delete_tracked",
                    key,
                    Some(prev_state),
                    Some(TrackState::Deleted),
                    None,
                );
            }
            TrackState::Deleted => {
                log_watched_storage_event(
                    "delete",
                    "apply_delete_already_deleted",
                    key,
                    Some(prev_state),
                    Some(TrackState::Deleted),
                    None,
                );
            }
            TrackState::NotFound => {
                let store_item = self.store_get.as_ref().and_then(|getter| getter(key));
                if store_item.is_none() {
                    log_watched_storage_event(
                        "delete",
                        "apply_delete_not_found_skip",
                        key,
                        Some(prev_state),
                        None,
                        None,
                    );
                    return;
                }

                let mut state = self.state.write();
                let current_state = state
                    .dictionary
                    .get(key)
                    .map_or(TrackState::NotFound, |t| t.state);
                match current_state {
                    TrackState::Added => {
                        state.dictionary.remove(key);
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_added",
                            key,
                            Some(prev_state),
                            Some(TrackState::NotFound),
                            store_item.as_ref(),
                        );
                    }
                    TrackState::Changed | TrackState::None | TrackState::NotFound => {
                        state.dictionary.insert(
                            key.clone(),
                            Trackable::new(StorageItem::default(), TrackState::Deleted),
                        );
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_emit",
                            key,
                            Some(prev_state),
                            Some(TrackState::Deleted),
                            store_item.as_ref(),
                        );
                    }
                    TrackState::Deleted => {
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_already_deleted",
                            key,
                            Some(prev_state),
                            Some(TrackState::Deleted),
                            store_item.as_ref(),
                        );
                    }
                }
            }
        }
    }

    /// Commits changes to the underlying store.
    pub fn commit(&self) {
        if self.read_only {
            return;
        }
        self.commit_writable();
    }

    fn commit_writable(&self) {
        if let Some(apply) = &self.commit_apply {
            let tracked = self.tracked_items();
            if !tracked.is_empty() {
                apply(&tracked);
            }
        }

        let mut state = self.state.write();
        state
            .dictionary
            .retain(|_, trackable| match trackable.state {
                TrackState::Added | TrackState::Changed => {
                    trackable.state = TrackState::None;
                    true
                }
                TrackState::Deleted => false,
                TrackState::None | TrackState::NotFound => true,
            });
    }

    /// Resets the cache for reuse.
    pub fn reset(&self) {
        let mut state = self.state.write();
        state.dictionary.clear();
        drop(state);
        if let Some(ref cache) = self.hot_key_cache {
            cache.clear();
        }
        if self.config.prefetch.enabled {
            *self.pattern_tracker.write() = AccessPatternTracker::new();
            self.prefetch_window.write().clear();
        }
    }
    /// Writes all modified entries directly to a byte store writer without intermediate cloning.
    pub fn apply_tracked_to_writer<W>(&self, writer: &mut W) -> crate::error::StorageResult<bool>
    where
        W: crate::persistence::write_store::WriteStore<Vec<u8>, Vec<u8>> + ?Sized,
    {
        let state = self.state.read();
        let mut any_written = false;
        for (key, trackable) in &state.dictionary {
            match trackable.state {
                TrackState::Added | TrackState::Changed => {
                    writer.put(key.to_array(), trackable.item.to_value())?;
                    any_written = true;
                }
                TrackState::Deleted => {
                    writer.delete(key.to_array())?;
                    any_written = true;
                }
                TrackState::None | TrackState::NotFound => {}
            }
        }
        Ok(any_written)
    }
}
