use super::*;
impl ReadOnlyStore for PersistenceDataCache {
    /// Overlay-first bounded scan.
    ///
    /// Backing rows are read through the bounded seek when one is installed, so
    /// the scan never materialises a prefix range larger than `limit`. Local
    /// writes and tombstones are then applied on top, which keeps this call
    /// consistent with [`Self::find`] without multiplying the read cost.
    fn seek_limited(
        &self,
        key_prefix: Option<&StorageKey>,
        start: Option<&StorageKey>,
        direction: SeekDirection,
        limit: usize,
    ) -> Vec<(StorageKey, StorageItem)> {
        if limit == 0 {
            return Vec::new();
        }

        let prefix_bytes = key_prefix.map(|key| key.as_bytes().into_owned());
        let inside_cursor = |key: &StorageKey| match direction {
            SeekDirection::Forward => start.is_none_or(|cursor| *key > *cursor),
            SeekDirection::Backward => start.is_none_or(|cursor| *key < *cursor),
        };

        let backing = match &self.store_seek {
            Some(seek) => seek(key_prefix, start, direction, limit),
            None => match &self.store_find {
                Some(find) => find(key_prefix, direction)
                    .into_iter()
                    .filter(|(key, _)| {
                        key_matches_prefix(key, prefix_bytes.as_deref()) && inside_cursor(key)
                    })
                    .take(limit)
                    .collect(),
                None => Vec::new(),
            },
        };

        let mut merged: BTreeMap<StorageKey, StorageItem> = backing.into_iter().collect();
        {
            let state = self.state.read();
            for (key, trackable) in &state.dictionary {
                if !key_matches_prefix(key, prefix_bytes.as_deref()) || !inside_cursor(key) {
                    continue;
                }
                match overlay_item(trackable) {
                    Some(Some(item)) => {
                        merged.insert(key.clone(), item);
                    }
                    Some(None) => {
                        merged.remove(key);
                    }
                    None => {}
                }
            }
        }

        // Keep unprefixed scans in C# byte order, consistent with `find`.
        if prefix_bytes.is_none() {
            let mut entries: Vec<_> = merged.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.compare_serialized(b));
            return match direction {
                SeekDirection::Forward => entries.into_iter().take(limit).collect(),
                SeekDirection::Backward => entries.into_iter().rev().take(limit).collect(),
            };
        }

        match direction {
            SeekDirection::Forward => merged.into_iter().take(limit).collect(),
            SeekDirection::Backward => merged.into_iter().rev().take(limit).collect(),
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for PersistenceDataCache {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        self.find(key_prefix, direction)
    }
}
