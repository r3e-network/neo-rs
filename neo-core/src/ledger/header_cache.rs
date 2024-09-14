use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use crate::block::Header;

/// Used to cache the headers of the blocks that have not been received.
pub struct HeaderCache {
    headers: Arc<RwLock<VecDeque<Header>>>,
}

impl HeaderCache {
    pub fn new() -> Self {
        HeaderCache {
            headers: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    /// Gets the `Header` at the specified index in the cache.
    ///
    /// # Arguments
    ///
    /// * `index` - The zero-based index of the `Header` to get.
    ///
    /// # Returns
    ///
    /// The `Header` at the specified index in the cache, or `None` if not found.
    pub fn get(&self, index: u32) -> Option<Header> {
        let headers = self.headers.read().unwrap();
        if headers.is_empty() {
            return None;
        }
        let first_index = headers[0].index();
        if index < *first_index {
            return None;
        }
        let adjusted_index = (index - first_index) as usize;
        if adjusted_index >= headers.len() {
            return None;
        }
        Some(headers[adjusted_index].clone())
    }

    /// Gets the number of elements in the cache.
    pub fn len(&self) -> usize {
        self.headers.read().unwrap().len()
    }

    /// Returns true if the cache contains no elements.
    pub fn is_empty(&self) -> bool {
        self.headers.read().unwrap().is_empty()
    }

    pub fn count(&self) -> usize {
        self.headers.read().unwrap().len()
    }

    /// Indicates whether the cache is full.
    pub fn is_full(&self) -> bool {
        self.count() >= 10000
    }

    /// Gets the last `Header` in the cache. Or `None` if the cache is empty.
    pub fn last(&self) -> Option<Header> {
        self.headers.read().unwrap().back().cloned()
    }

    /// Adds a header to the cache.
    pub fn add(&self, header: Header) {
        self.headers.write().unwrap().push_back(header);
    }

    /// Tries to remove the first header from the cache.
    ///
    /// # Returns
    ///
    /// A tuple containing a boolean indicating success and the removed header (if any).
    pub fn try_remove_first(&self) -> (bool, Option<Header>) {
        let mut headers = self.headers.write().unwrap();
        if let Some(header) = headers.pop_front() {
            (true, Some(header))
        } else {
            (false, None)
        }
    }
}

impl IntoIterator for HeaderCache {
    type Item = Header;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.headers.read().unwrap().clone().into_iter().collect::<Vec<_>>().into_iter()
    }
}

impl<'a> IntoIterator for &'a HeaderCache {
    type Item = Header;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.headers.read().unwrap().clone().into_iter().collect::<Vec<_>>().into_iter()
    }
}
