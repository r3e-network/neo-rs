use crate::IoResult;
use lru::LruCache;
use std::hash::Hash;
use std::num::NonZeroUsize;

pub(crate) struct OrderedCache<TKey, TValue>
where
    TKey: Eq + Hash,
{
    entries: Option<LruCache<TKey, TValue>>,
}

fn check_copy_range(
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

impl<TKey, TValue> OrderedCache<TKey, TValue>
where
    TKey: Eq + Hash,
{
    pub(crate) fn new(max_capacity: usize) -> Self {
        Self {
            entries: NonZeroUsize::new(max_capacity).map(LruCache::new),
        }
    }

    pub(crate) fn resize(&mut self, max_capacity: usize) {
        let Some(capacity) = NonZeroUsize::new(max_capacity) else {
            self.entries = None;
            return;
        };

        match self.entries.as_mut() {
            Some(entries) => entries.resize(capacity),
            None => self.entries = Some(LruCache::new(capacity)),
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

    pub(crate) fn contains(&self, key: &TKey) -> bool {
        self.entries
            .as_ref()
            .is_some_and(|entries| entries.contains(key))
    }

    pub(crate) fn touch(&mut self, key: &TKey) -> bool {
        self.entries
            .as_mut()
            .is_some_and(|entries| entries.get(key).is_some())
    }

    pub(crate) fn insert_if_absent(&mut self, key: TKey, value: TValue) {
        let Some(entries) = self.entries.as_mut() else {
            return;
        };

        if entries.contains(&key) {
            return;
        }

        entries.put(key, value);
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

    pub(crate) fn copy_keys_to(&self, destination: &mut [TKey], start_index: usize) -> IoResult<()>
    where
        TKey: Clone,
    {
        check_copy_range("copy_to", start_index, self.len(), destination.len())?;

        if let Some(entries) = self.entries.as_ref() {
            for (offset, key) in entries.iter().rev().map(|(key, _)| key.clone()).enumerate() {
                destination[start_index + offset] = key;
            }
        }

        Ok(())
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &TKey> {
        self.entries
            .as_ref()
            .into_iter()
            .flat_map(|entries| entries.iter().rev().map(|(key, _)| key))
    }

    pub(crate) fn peek_cloned(&self, key: &TKey) -> Option<TValue>
    where
        TValue: Clone,
    {
        self.entries
            .as_ref()
            .and_then(|entries| entries.peek(key).cloned())
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
