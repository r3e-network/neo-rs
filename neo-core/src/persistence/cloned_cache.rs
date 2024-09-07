

pub struct ClonedCache {
    inner_cache: Box<dyn DataCache>,
}

impl ClonedCache {
    pub fn new(inner_cache: Box<dyn DataCache>) -> Self {
        Self { inner_cache }
    }
}

impl DataCache for ClonedCache {
    fn add(&mut self, key: StorageKey, value: StorageItem) -> Result<(), Error> {
        self.inner_cache.add(key, value.clone())
    }

    fn delete(&mut self, key: &StorageKey) -> Result<(), Error> {
        self.inner_cache.delete(key)
    }

    fn contains(&self, key: &StorageKey) -> Result<bool, Error> {
        self.inner_cache.contains(key)
    }

    fn get(&self, key: &StorageKey) -> Result<StorageItem, Error> {
        self.inner_cache.get(key).map(|item| item.clone())
    }

    fn seek(&self, key_or_prefix: &[u8], direction: SeekDirection) -> Result<Vec<(StorageKey, StorageItem)>, Error> {
        self.inner_cache.seek(key_or_prefix, direction)
            .map(|items| items.into_iter().map(|(k, v)| (k, v.clone())).collect())
    }

    fn try_get(&self, key: &StorageKey) -> Result<Option<StorageItem>, Error> {
        self.inner_cache.try_get(key).map(|opt| opt.map(|item| item.clone()))
    }

    fn update(&mut self, key: &StorageKey, value: StorageItem) -> Result<(), Error> {
        let mut current = self.inner_cache.get_and_change(key)?;
        current.from_replica(&value);
        Ok(())
    }
}
