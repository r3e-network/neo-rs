use super::*;
impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        // Check read cache first
        // Note: Vec<u8> doesn't implement StorageKey, so we skip caching for raw bytes
        match self.db.get(key) {
            Ok(value) => value,
            Err(err) => fail_fast_read("get", &err),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        if direction == SeekDirection::Backward
            && let Some(prefix) = key_prefix
        {
            let iterator = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                None,
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            return Box::new(iterator.filter_map(|res| match res {
                Ok((key, value)) => Some((key.to_vec(), value.to_vec())),
                Err(err) => fail_fast_read("iterator", &err),
            }));
        }

        let start = key_prefix.map(Vec::as_slice).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        let prefix = key_prefix.cloned();
        let iterator = iterator.take_while(move |res| match res {
            Ok((key, _)) => prefix.as_ref().is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });
        Box::new(iterator.filter_map(|res| match res {
            Ok((k, v)) => Some((k.to_vec(), v.to_vec())),
            Err(err) => fail_fast_read("iterator", &err),
        }))
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        // Capture the generation before the value is read. A cache fill may
        // only publish it while this generation is still current.
        let generation = self.read_cache_generation();

        // Check read cache first (bloom filter will be checked inside).
        if let Some(ref cache) = self.read_cache {
            let _gate = self.cache_gate.read();
            if let Some(item) = cache.get(key) {
                return Some(item);
            }
        }

        let raw = key.to_array();
        let result = match self.db.get(&raw) {
            Ok(Some(bytes)) => Some(StorageItem::from_bytes(bytes)),
            Ok(None) => None,
            Err(err) => fail_fast_read("get", &err),
        };

        if let Some(item) = &result {
            self.fill_read_cache(key, item, generation);
        }

        result
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(StorageKey::to_array);

        // Capture the generation before iterating so a scan that raced a commit
        // cannot publish pre-commit rows into the shared cache.
        let generation = self.read_cache_generation();
        let read_cache = self.read_cache.clone();
        let gate = Arc::clone(&self.cache_gate);
        let cache_generation = Arc::clone(&self.cache_generation);

        if direction == SeekDirection::Backward
            && let Some(prefix) = prefix_bytes.as_ref()
        {
            let iter = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                None,
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            return Box::new(iter.map(move |res| {
                let (key, value) = match res {
                    Ok(entry) => entry,
                    Err(err) => fail_fast_read("iterator", &err),
                };
                let key_vec: Vec<u8> = key.into();
                let storage_key = StorageKey::from_bytes(&key_vec);
                let storage_item = StorageItem::from_bytes(value.into());
                fill_read_cache_if_current(
                    read_cache.as_deref(),
                    &gate,
                    &cache_generation,
                    generation,
                    &storage_key,
                    &storage_item,
                );
                (storage_key, storage_item)
            }));
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);
        let prefix_boundary = prefix_bytes.clone();
        let iter = iter.take_while(move |res| match res {
            Ok((key, _)) => prefix_boundary
                .as_ref()
                .is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });

        // Create an iterator that also caches results
        Box::new(iter.filter_map(move |res| {
            let (key, value) = match res {
                Ok(entry) => entry,
                Err(err) => fail_fast_read("iterator", &err),
            };
            let key_vec: Vec<u8> = key.into();
            if let Some(prefix) = &prefix_bytes
                && !key_vec.starts_with(prefix)
            {
                return None;
            }
            let storage_key = StorageKey::from_bytes(&key_vec);
            let storage_item = StorageItem::from_bytes(value.into());

            // Cache the result only if no commit raced this scan.
            fill_read_cache_if_current(
                read_cache.as_deref(),
                &gate,
                &cache_generation,
                generation,
                &storage_key,
                &storage_item,
            );

            Some((storage_key, storage_item))
        }))
    }
}

impl ReadOnlyStore for RocksDbStore {
    /// Bounded prefix scan used by speculative readers.
    ///
    /// Unlike [`ReadOnlyStoreGeneric::find`] this never builds an unbounded
    /// result set: iteration stops at `limit` rows and at the namespace
    /// boundary, so a prefetch over a large prefix costs one iterator seek plus
    /// at most `limit` row reads.
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

        let prefix_bytes = key_prefix.map(StorageKey::to_array);
        // The cursor only narrows the scan while it lies inside the namespace.
        let cursor = start
            .filter(|cursor| {
                prefix_bytes
                    .as_ref()
                    .is_none_or(|prefix| cursor.as_bytes().starts_with(prefix))
            })
            .cloned();
        let cursor_bytes = cursor.as_ref().map(StorageKey::to_array);

        let mut entries = Vec::with_capacity(limit.min(64));
        let push = |key_vec: Vec<u8>, value: Vec<u8>, entries: &mut Vec<_>| {
            if let Some(prefix) = prefix_bytes.as_deref()
                && !key_vec.starts_with(prefix)
            {
                return false;
            }
            let storage_key = StorageKey::from_bytes(&key_vec);
            if let Some(cursor) = cursor.as_ref() {
                let outside = match direction {
                    SeekDirection::Forward => storage_key <= *cursor,
                    SeekDirection::Backward => storage_key >= *cursor,
                };
                if outside {
                    return true;
                }
            }
            entries.push((storage_key, StorageItem::from_bytes(value)));
            true
        };

        match direction {
            SeekDirection::Forward => {
                let seek = cursor_bytes
                    .as_deref()
                    .or(prefix_bytes.as_deref())
                    .unwrap_or(&[]);
                for res in self.iterator_from(seek, SeekDirection::Forward) {
                    let (key, value) = match res {
                        Ok(entry) => entry,
                        Err(err) => fail_fast_read("iterator", &err),
                    };
                    if !push(key.to_vec(), value.to_vec(), &mut entries) {
                        break;
                    }
                    if entries.len() == limit {
                        break;
                    }
                }
            }
            SeekDirection::Backward => {
                let iter = provider::reverse_prefix_iterator_from(
                    self.db.as_ref(),
                    None,
                    prefix_bytes.as_deref().unwrap_or(&[]),
                    cursor_bytes.as_deref(),
                    &self.read_ahead_config,
                );
                for res in iter {
                    let (key, value) = match res {
                        Ok(entry) => entry,
                        Err(err) => fail_fast_read("iterator", &err),
                    };
                    if !push(key.to_vec(), value.to_vec(), &mut entries) {
                        break;
                    }
                    if entries.len() == limit {
                        break;
                    }
                }
            }
        }

        entries
    }
}
