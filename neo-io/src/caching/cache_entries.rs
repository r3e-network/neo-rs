use crate::IoResult;
use indexmap::IndexMap;
use lru::LruCache;
use std::hash::Hash;
use std::num::NonZeroUsize;

pub(crate) struct FifoEntries<TKey, TValue>
where
    TKey: Eq + Hash,
{
    max_capacity: usize,
    entries: IndexMap<TKey, TValue>,
}

pub(crate) fn check_copy_range(
    context: &'static str,
    start_index: usize,
    count: usize,
    destination_len: usize,
) -> IoResult<()> {
    if start_index > destination_len {
        return Err(crate::IoError::InvalidData {
            context: context.to_string(),
            value: format!(
                "start_index ({}) exceeds destination length ({})",
                start_index, destination_len
            ),
        });
    }

    let end_index = start_index
        .checked_add(count)
        .ok_or_else(|| crate::IoError::InvalidData {
            context: context.to_string(),
            value: format!("start_index ({start_index}) + count ({count}) overflows"),
        })?;
    if end_index > destination_len {
        return Err(crate::IoError::InvalidData {
            context: context.to_string(),
            value: format!(
                "start_index ({}) + count ({}) > destination length ({})",
                start_index, count, destination_len
            ),
        });
    }

    Ok(())
}

impl<TKey, TValue> FifoEntries<TKey, TValue>
where
    TKey: Eq + Hash,
{
    pub(crate) fn new(max_capacity: usize) -> Self {
        Self {
            max_capacity,
            entries: IndexMap::with_capacity(max_capacity),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn contains(&self, key: &TKey) -> bool {
        self.entries.contains_key(key)
    }

    pub(crate) fn insert_if_absent(&mut self, key: TKey, value: TValue) {
        if self.max_capacity == 0 || self.entries.contains_key(&key) {
            return;
        }

        if self.entries.len() == self.max_capacity {
            self.entries.shift_remove_index(0);
        }

        self.entries.insert(key, value);
    }

    pub(crate) fn remove(&mut self, key: &TKey) -> bool {
        self.entries.shift_remove(key).is_some()
    }

    pub(crate) fn copy_to(&self, destination: &mut [TValue], start_index: usize) -> IoResult<()>
    where
        TValue: Clone,
    {
        check_copy_range("copy_to", start_index, self.len(), destination.len())?;

        for (offset, value) in self.entries.values().cloned().enumerate() {
            destination[start_index + offset] = value;
        }

        Ok(())
    }

    pub(crate) fn peek_cloned(&self, key: &TKey) -> Option<TValue>
    where
        TValue: Clone,
    {
        self.entries.get(key).cloned()
    }

    pub(crate) fn values(&self) -> Vec<TValue>
    where
        TValue: Clone,
    {
        self.entries.values().cloned().collect()
    }
}

pub(crate) struct LruEntries<TKey, TValue>
where
    TKey: Eq + Hash,
{
    entries: Option<LruCache<TKey, TValue>>,
}

impl<TKey, TValue> LruEntries<TKey, TValue>
where
    TKey: Eq + Hash,
{
    pub(crate) fn new(max_capacity: usize) -> Self {
        Self {
            entries: NonZeroUsize::new(max_capacity).map(LruCache::new),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.as_ref().map_or(0, LruCache::len)
    }

    pub(crate) fn clear(&mut self) {
        if let Some(entries) = self.entries.as_mut() {
            entries.clear();
        }
    }

    pub(crate) fn touch(&mut self, key: &TKey) -> bool {
        self.entries
            .as_mut()
            .is_some_and(|entries| entries.get(key).is_some())
    }

    pub(crate) fn insert_or_touch(&mut self, key: TKey, value: TValue) {
        let Some(entries) = self.entries.as_mut() else {
            return;
        };

        if entries.get(&key).is_some() {
            return;
        }

        entries.put(key, value);
    }

    pub(crate) fn remove(&mut self, key: &TKey) -> bool {
        self.entries
            .as_mut()
            .is_some_and(|entries| entries.pop(key).is_some())
    }

    pub(crate) fn copy_to(&self, destination: &mut [TValue], start_index: usize) -> IoResult<()>
    where
        TValue: Clone,
    {
        check_copy_range("copy_to", start_index, self.len(), destination.len())?;

        if let Some(entries) = self.entries.as_ref() {
            for (offset, value) in entries
                .iter()
                .rev()
                .map(|(_, value)| value.clone())
                .enumerate()
            {
                destination[start_index + offset] = value;
            }
        }

        Ok(())
    }

    pub(crate) fn get_cloned(&mut self, key: &TKey) -> Option<TValue>
    where
        TValue: Clone,
    {
        self.entries
            .as_mut()
            .and_then(|entries| entries.get(key).cloned())
    }

    pub(crate) fn values(&self) -> Vec<TValue>
    where
        TValue: Clone,
    {
        self.entries
            .as_ref()
            .map(|entries| {
                entries
                    .iter()
                    .rev()
                    .map(|(_, value)| value.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}
